// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Admission of a round, and the checks every proposer's plan passes: the
//! frame's critical path on each device and the memory held against what is
//! known of it.

use khora_core::agent::gorna::protocol::{Allocation, BindingConstraint, Envelope, Trigger};
use khora_core::agent::gorna::{AdaptationMode, AgentId};
use khora_core::lane::cost::{Device, PerDevice};

use super::plan::Plan;
use super::round::{Bid, Round};
use super::share::{name_priced, priced};
use super::units::{fits_envelope, micros, transfer_micros};

/// What admission gives the proposers: the bids in one order, and what it
/// planned for each — or the floors, when they do not fit.
pub(crate) enum Admission<'r> {
    /// The floors do not fit, or the round opened as `Safety`: every agent is
    /// lent its floor, and the breach the floors made, if they made one.
    Safety(Allocation, Option<BindingConstraint>),
    /// Every floor fits.
    Admitted {
        /// The bids, ordered by agent.
        bids: Vec<&'r Bid>,
        /// What admission planned for each, in the same order.
        plans: Vec<Plan>,
    },
}

/// Admits `round`: one order for everything that follows, every agent's
/// floor checked on the critical path, and a `Stable` agent's envelope kept
/// while it still fits the frame with every other agent at its floor.
pub(crate) fn admit<'r>(round: &'r Round<'_>) -> Admission<'r> {
    let (bids, mut plans) = floors(round);
    let breached = breach(round, &bids, &plans);
    if round.trigger == Trigger::Safety || breached.is_some() {
        let mut allocation = Allocation {
            epoch: round.epoch,
            trigger: Trigger::Safety,
            envelopes: bids
                .iter()
                .zip(&plans)
                .map(|(bid, plan)| {
                    let floor = Envelope {
                        time: plan.floor.time,
                        memory: plan.floor.memory,
                    };
                    // An agent that offered nothing this round (busy, or
                    // late with no rows) has no floor to fall to: it keeps
                    // what it holds.
                    // One that priced no row has a floor of no known cost:
                    // it keeps what it holds too, or is named in no envelope.
                    let keeps = bid.offer.rows.is_empty() || !priced(bid);
                    (
                        bid.offer.agent,
                        plan.fixed.filter(|_| keeps).unwrap_or(floor),
                    )
                })
                .collect(),
            pins: Vec::new(),
            allowed: Vec::new(),
            late: late(&bids),
            price: None,
            // Every agent is lent its floor whatever the target.
            lever: false,
        };
        // An agent that priced no row has no floor known to lend.
        name_priced(&mut allocation.envelopes, &bids, &plans);
        // A round opened as `Safety` names no breach of its own.
        let binding = breached.filter(|_| round.trigger != Trigger::Safety);
        return Admission::Safety(allocation, binding);
    }

    // A `Stable` agent keeps the envelope it holds while it still fits the
    // frame with every other agent at its floor, and takes no upgrade; one
    // that no longer fits gives way, and may not grow past it.
    for index in 0..plans.len() {
        if plans[index].fixed.is_some() || bids[index].mode != AdaptationMode::Stable {
            continue;
        }
        if let Some(kept) = bids[index].kept {
            // An envelope too small for the floor the agent offers now is
            // not kept: it would lend the agent less than it can run.
            if !fits_envelope(&plans[index].floor, &kept) {
                plans[index].ceiling = Some(kept);
                continue;
            }
            plans[index].fixed = Some(kept);
            if breach(round, &bids, &plans).is_some() {
                plans[index].fixed = None;
                plans[index].ceiling = Some(kept);
            }
        }
    }
    Admission::Admitted { bids, plans }
}

/// `round`'s bids in agent order, and each one's plan at its floor, as its
/// policies make it.
pub(crate) fn floors<'r>(round: &'r Round<'_>) -> (Vec<&'r Bid>, Vec<Plan>) {
    let mut bids: Vec<&Bid> = round.bids.iter().collect();
    bids.sort_by_key(|bid| bid.offer.agent);
    let plans = bids.iter().map(|bid| Plan::of(bid)).collect();
    (bids, plans)
}

/// What the plans leave of the target, in milliseconds, on the device whose
/// critical path is the longest: negative when they are over it.
pub(crate) fn slack_ms(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> f64 {
    let paths = critical_paths(round, bids, plans);
    let longest = Device::ALL
        .into_iter()
        .map(|device| device.get(&paths))
        .max()
        .unwrap_or_default();
    (micros(round.target) as f64 - longest as f64) / 1_000.0
}

/// The agents named late in `bids`.
pub(super) fn late(bids: &[&Bid]) -> Vec<AgentId> {
    bids.iter()
        .filter(|bid| bid.late)
        .map(|bid| bid.offer.agent)
        .collect()
}

/// What the plans break, if anything: the first device whose critical path
/// is over the target, else the first whose memory held is over what is
/// known of it. `None` when they fit the frame.
pub(crate) fn breach(
    round: &Round<'_>,
    bids: &[&Bid],
    plans: &[Plan],
) -> Option<BindingConstraint> {
    let target = micros(round.target);
    let paths = critical_paths(round, bids, plans);
    if let Some(device) = Device::ALL
        .into_iter()
        .find(|device| device.get(&paths) > target)
    {
        return Some(BindingConstraint::Time(device));
    }
    let held = held(bids, plans);
    Device::ALL
        .into_iter()
        .find(|device| {
            device
                .get(&round.memory)
                .is_some_and(|limit| device.get(&held) > limit)
        })
        .map(BindingConstraint::Memory)
}

/// The frame's time on each device, in µs: each wave costs its slowest
/// member, an agent in no wave its own, and every agent its transfers.
pub(crate) fn critical_paths(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> PerDevice<u64> {
    let mut alone = PerDevice::<u64>::default();
    let mut waves = vec![PerDevice::<u64>::default(); round.waves.len()];
    for (bid, plan) in bids.iter().zip(plans) {
        // Off the frame: its time is the free threads', not the frame's.
        if round.background.contains(&bid.offer.agent) {
            continue;
        }
        let (time, _, transfer) = plan.asks(&bid.offer.rows);
        let moving = transfer_micros(&transfer, &round.link_rate);
        let wave = round
            .waves
            .iter()
            .position(|wave| wave.contains(&bid.offer.agent));
        for device in Device::ALL {
            let cost = device.get(&time).saturating_add(moving);
            match wave {
                Some(wave) => {
                    let slowest = device.get_mut(&mut waves[wave]);
                    *slowest = (*slowest).max(cost);
                }
                None => {
                    let sum = device.get_mut(&mut alone);
                    *sum = sum.saturating_add(cost);
                }
            }
        }
    }
    for wave in waves {
        for device in Device::ALL {
            let sum = device.get_mut(&mut alone);
            *sum = sum.saturating_add(device.get(&wave));
        }
    }
    alone
}

/// The memory the plans hold on each device.
pub(super) fn held(bids: &[&Bid], plans: &[Plan]) -> PerDevice<u64> {
    let mut held = PerDevice::<u64>::default();
    for (bid, plan) in bids.iter().zip(plans) {
        let (_, memory, _) = plan.asks(&bid.offer.rows);
        for device in Device::ALL {
            let sum = device.get_mut(&mut held);
            *sum = sum.saturating_add(device.get(&memory));
        }
    }
    held
}
