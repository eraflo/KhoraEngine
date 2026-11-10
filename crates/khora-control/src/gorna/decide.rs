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

//! How the DCC shares the frame: from every agent's offer, one envelope each.
//!
//! [`decide`] is a pure function of its [`Round`]: the same round always gives
//! the same [`Allocation`]. The [`Arbiter`] decides it; this module holds
//! what every proposer shares — the round, admission, the checks and the
//! share.

use std::time::Duration;

use khora_core::agent::gorna::protocol::{
    Allocation, Binding, Confidence, Envelope, Epoch, Offer, OfferRow, Trigger,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, StrategyId};
use khora_core::agent::AgentImportance;
use khora_core::lane::cost::{Cost, Device, PerDevice, PerLink};

use super::arbiter::Arbiter;

/// One agent's part of a round: its offer, and what the DCC holds for it.
#[derive(Debug, Clone)]
pub struct Bid {
    /// The agent's offer.
    pub offer: Offer,
    /// How much the agent weighs: Critical 1.0, Important 0.75, Optional 0.5,
    /// unless a `Prioritize` hint in [`hints`](Self::hints) replaces it.
    pub importance: AgentImportance,
    /// The developer's policy: `Manual` pins a row, `Bounded` allows a range
    /// of rows, `Stable` keeps the envelope, `Learning` leaves the DCC free.
    pub mode: AdaptationMode,
    /// The developer's hints: `cap_ms` is a ceiling on the envelope's time,
    /// `priority` replaces the importance's weight.
    pub hints: AgentHints,
    /// The envelope the agent holds from the allocation in force, if any.
    pub kept: Option<Envelope>,
    /// Whether the offer came after the request's `reply_by`: the agent then
    /// keeps [`kept`](Self::kept) and is named in [`Allocation::late`].
    pub late: bool,
}

/// What a round of GORNA decides from.
#[derive(Debug, Clone)]
pub struct Round<'a> {
    /// The epoch the round opens.
    pub epoch: Epoch,
    /// Why the round opens. `Trigger::Safety` gives every agent its floor.
    pub trigger: Trigger,
    /// The frame's time on the critical path, on every device.
    pub target: Duration,
    /// The bytes the agents may hold on each device; `None` where it is not
    /// known, which limits nothing.
    pub memory: PerDevice<Option<u64>>,
    /// The bytes per second each link moves; `0` where a link's transfers are
    /// not timed.
    pub link_rate: PerLink<u64>,
    /// The scheduler's waves: the agents of one wave run side by side, so a
    /// wave costs its slowest member. An agent in no wave runs alone.
    pub waves: &'a [Vec<AgentId>],
    /// The agents that run off the frame, at
    /// [`ExecutionPhase::BACKGROUND`](khora_core::agent::ExecutionPhase::BACKGROUND):
    /// they add nothing to the frame's critical path.
    pub background: &'a [AgentId],
    /// Every agent asked, with its offer.
    pub bids: &'a [Bid],
}

/// The allocation for `round`, as the default [`Arbiter`] decides it.
///
/// 1. **Admission.** Every agent's floor must fit the target on the critical
///    path (on every device, the waves' slowest members plus the transfers);
///    otherwise the allocation is `Trigger::Safety`, every envelope its
///    agent's floor.
/// 2. **The plan.** The Q-RAM greedy takes upgrades over each agent's convex
///    hull in order of `Δ(weight × quality) / Δcost` (cost in µs) while they
///    fit; the exact search starts from its plan and lends a plan of more
///    weighed quality when it finds one within its node budget.
/// 3. **Envelopes.** Each agent gets the cost of the row planned for it, plus
///    80 % of the slack shared in proportion to that cost; 20 % stays in
///    reserve.
///
/// Times are counted in whole microseconds.
pub fn decide(round: &Round<'_>) -> Allocation {
    Arbiter::default().decide(round).allocation
}

/// What admission gives the proposers: the bids in one order, and what it
/// planned for each — or the floors, when they do not fit.
pub(crate) enum Admission<'r> {
    /// The floors do not fit, or the round opened as `Safety`: every agent is
    /// lent its floor, and the breach the floors made, if they made one.
    Safety(Allocation, Option<Binding>),
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
        let allocation = Allocation {
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
                    let offered_nothing = bid.offer.rows.is_empty();
                    (
                        bid.offer.agent,
                        plan.fixed.filter(|_| offered_nothing).unwrap_or(floor),
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
fn late(bids: &[&Bid]) -> Vec<AgentId> {
    bids.iter()
        .filter(|bid| bid.late)
        .map(|bid| bid.offer.agent)
        .collect()
}

/// The allocation that lends `plans`: the envelopes shared from them, the
/// pins and allowed rows the policies name, the agents late, the price of
/// the last upgrade, and whether a lower target can take something back.
pub(crate) fn allocation(
    round: &Round<'_>,
    bids: &[&Bid],
    plans: &[Plan],
    price: Option<f32>,
) -> Allocation {
    let envelopes = share(round, bids, plans);
    let lever = bids
        .iter()
        .zip(plans)
        .zip(&envelopes)
        .any(|((bid, plan), (_, envelope))| above_floor(bid, plan, envelope));
    Allocation {
        epoch: round.epoch,
        trigger: round.trigger,
        envelopes,
        pins: bids
            .iter()
            .filter_map(|bid| match bid.mode {
                AdaptationMode::Manual(id) if bid.offer.rows.iter().any(|row| row.id == id) => {
                    Some((bid.offer.agent, id))
                }
                _ => None,
            })
            .collect(),
        allowed: bids
            .iter()
            .filter_map(|bid| match bid.mode {
                AdaptationMode::Bounded { min, max } => Some((
                    bid.offer.agent,
                    bid.offer
                        .rows
                        .iter()
                        .filter(|row| within(row.id, min, max))
                        .map(|row| row.id)
                        .collect(),
                )),
                _ => None,
            })
            .collect(),
        late: late(bids),
        price,
        lever,
    }
}

/// Whether a lower frame target can take something back from `bid`: a row
/// above its floor fits the envelope it is lent, so a smaller one makes it
/// run a cheaper row. That covers a planned upgrade, a `Stable` envelope it
/// keeps, and the slack share every envelope carries — an agent chooses its
/// row within its envelope, whatever row the plan started it on. No target
/// moves a row a `Manual` policy pins, nor what a late or busy agent keeps.
fn above_floor(bid: &Bid, plan: &Plan, envelope: &Envelope) -> bool {
    if matches!(bid.mode, AdaptationMode::Manual(_)) || bid.late || bid.offer.rows.is_empty() {
        return false;
    }
    let floor = busiest(&plan.floor.time);
    bid.offer
        .rows
        .iter()
        .filter(|row| busiest(&row.cost.time) > floor)
        .any(|row| fits_envelope(&row.cost, envelope))
}

/// What the round plans for one agent.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The floor's cost: every envelope holds at least it.
    pub(crate) floor: Cost,
    /// The row planned, as an index into the offer's rows; `None` when the
    /// agent offers no floor row.
    pub(crate) row: Option<usize>,
    /// An envelope the agent keeps whatever the share: a late agent's, or a
    /// `Stable` agent's that still fits.
    pub(crate) fixed: Option<Envelope>,
    /// An envelope a `Stable` agent's may not grow past.
    pub(crate) ceiling: Option<Envelope>,
    /// The rows ruled out: unpriced, outside a `Bounded` range, over the cap,
    /// or any row of a pinned agent; a proposer also rules out the rows it
    /// finds not to fit.
    pub(crate) out: Vec<bool>,
}

impl Plan {
    fn of(bid: &Bid) -> Self {
        let rows = &bid.offer.rows;
        let floor_row = rows.iter().position(|row| row.id == bid.offer.floor);
        let floor = floor_row.map_or_else(Cost::default, |row| rows[row].cost);
        let cap = bid.hints.cap_ms.map(micros_of_ms);
        let pinned = match bid.mode {
            AdaptationMode::Manual(id) => rows.iter().position(|row| row.id == id),
            _ => None,
        };
        // A `Bounded` range that leaves the floor out starts the agent on the
        // cheapest priced row it allows: the floor is not one it may run.
        let bounded_base = match bid.mode {
            AdaptationMode::Bounded { min, max }
                if floor_row.is_none_or(|row| !within(rows[row].id, min, max)) =>
            {
                rows.iter()
                    .enumerate()
                    .filter(|(_, row)| {
                        within(row.id, min, max) && row.confidence != Confidence::Assumed
                    })
                    .min_by_key(|(_, row)| busiest(&row.cost.time))
                    .map(|(index, _)| index)
            }
            _ => None,
        };
        let out = rows
            .iter()
            .map(|row| {
                pinned.is_some()
                    || row.confidence == Confidence::Assumed
                    || match bid.mode {
                        AdaptationMode::Bounded { min, max } => !within(row.id, min, max),
                        _ => false,
                    }
                    || cap.is_some_and(|cap| micros(busiest(&row.cost.time)) > cap)
            })
            .collect();
        Self {
            floor,
            row: pinned.or(bounded_base).or(floor_row),
            fixed: if bid.late { bid.kept } else { None },
            ceiling: None,
            out,
        }
    }

    /// What the plan asks of the frame: the time on each device (µs), the
    /// memory held on each, and the bytes moved on each link. A planned row
    /// never asks less than the floor. A kept envelope asks what it lent: its
    /// time, and its memory on every device where that memory was known — an
    /// envelope lent while a device's memory was not known reads `u64::MAX`
    /// there, and then asks its rows' memory instead.
    fn asks(&self, rows: &[OfferRow]) -> (PerDevice<u64>, PerDevice<u64>, PerLink<u64>) {
        let planned = self.row.map_or(self.floor, |row| rows[row].cost);
        let mut time = PerDevice::default();
        let mut memory = PerDevice::default();
        for device in Device::ALL {
            *device.get_mut(&mut time) =
                micros(device.get(&planned.time)).max(micros(device.get(&self.floor.time)));
            let rows_memory = device
                .get(&planned.memory)
                .max(device.get(&self.floor.memory));
            *device.get_mut(&mut memory) = match self.fixed {
                Some(fixed) if device.get(&fixed.memory) != u64::MAX => device.get(&fixed.memory),
                _ => rows_memory,
            };
        }
        if let Some(fixed) = self.fixed {
            return (each(&fixed.time, micros), memory, PerLink::default());
        }
        (time, memory, planned.transfer)
    }
}

/// What the plans break, if anything: the first device whose critical path
/// is over the target, else the first whose memory held is over what is
/// known of it. `None` when they fit the frame.
pub(crate) fn breach(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> Option<Binding> {
    let target = micros(round.target);
    let paths = critical_paths(round, bids, plans);
    if let Some(device) = Device::ALL
        .into_iter()
        .find(|device| device.get(&paths) > target)
    {
        return Some(Binding::Time(device));
    }
    let held = held(bids, plans);
    Device::ALL
        .into_iter()
        .find(|device| {
            device
                .get(&round.memory)
                .is_some_and(|limit| device.get(&held) > limit)
        })
        .map(Binding::Memory)
}

/// How much `bid` weighs: its `Prioritize` hint, else its importance —
/// Critical 1.0, Important 0.75, Optional 0.5.
pub(crate) fn weight(bid: &Bid) -> f64 {
    f64::from(bid.hints.priority.unwrap_or(match bid.importance {
        AgentImportance::Critical => 1.0,
        AgentImportance::Important => 0.75,
        AgentImportance::Optional => 0.5,
    }))
}

/// The frame's time on each device, in µs: each wave costs its slowest
/// member, an agent in no wave its own, and every agent its transfers.
fn critical_paths(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> PerDevice<u64> {
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
fn held(bids: &[&Bid], plans: &[Plan]) -> PerDevice<u64> {
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

/// The envelopes: each agent's planned time and memory, plus 80 % of what
/// the frame leaves on each device shared in proportion to them; 20 % stays
/// in reserve. A kept envelope is lent as it is. A cap and a `Stable` ceiling
/// bound the time, never below the floor. A memory the DCC does not know
/// limits nothing.
///
/// The share scales every planned time by one factor per device, so a wave's
/// slowest member stays its slowest and the critical path grows by 80 % of
/// the slack at most.
fn share(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> Vec<(AgentId, Envelope)> {
    let target = micros(round.target);
    let paths = critical_paths(round, bids, plans);
    let held = held(bids, plans);
    bids.iter()
        .zip(plans)
        .map(|(bid, plan)| {
            let (time, memory, _) = plan.asks(&bid.offer.rows);
            let mut envelope = Envelope::default();
            for device in Device::ALL {
                let asked = device.get(&time);
                let path = device.get(&paths);
                let extra = eighty_percent_share(target.saturating_sub(path), asked, path);
                let mut lent = asked.saturating_add(extra);
                if let Some(cap) = bid.hints.cap_ms.map(micros_of_ms) {
                    lent = lent.min(cap.max(micros(device.get(&plan.floor.time))));
                }
                if let Some(ceiling) = &plan.ceiling {
                    let bound = micros(device.get(&ceiling.time));
                    lent = lent.min(bound.max(micros(device.get(&plan.floor.time))));
                }
                *device.get_mut(&mut envelope.time) = Duration::from_micros(lent);
                *device.get_mut(&mut envelope.memory) = match device.get(&round.memory) {
                    None => u64::MAX,
                    Some(limit) => {
                        let (asked, total) = (device.get(&memory), device.get(&held));
                        asked.saturating_add(eighty_percent_share(
                            limit.saturating_sub(total),
                            asked,
                            total,
                        ))
                    }
                };
            }
            // A kept envelope is lent as it was, except for memory it can no
            // longer have now the DCC knows the device's.
            // A kept envelope is lent as it was. Where it was lent while the
            // device's memory was not known (`u64::MAX`), its memory is the
            // share computed now.
            if let Some(fixed) = plan.fixed {
                envelope.time = fixed.time;
                for device in Device::ALL {
                    if device.get(&fixed.memory) != u64::MAX {
                        *device.get_mut(&mut envelope.memory) = device.get(&fixed.memory);
                    }
                }
            }
            (bid.offer.agent, envelope)
        })
        .collect()
}

/// `asked`'s part of 80 % of `slack`, in proportion to `asked` out of
/// `total`, rounded down.
fn eighty_percent_share(slack: u64, asked: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    (u128::from(slack) * 8 * u128::from(asked) / (10 * u128::from(total))) as u64
}

/// Whether `cost` fits `envelope`, its time and memory on every device.
pub(crate) fn fits_envelope(cost: &Cost, envelope: &Envelope) -> bool {
    Device::ALL.iter().all(|device| {
        device.get(&cost.time) <= device.get(&envelope.time)
            && device.get(&cost.memory) <= device.get(&envelope.memory)
    })
}

/// Whether `id` lies in the `Bounded` range `min..=max`, the ids ordered
/// `LowPower < Balanced < HighPerformance < Custom(n)` by `n`.
fn within(id: StrategyId, min: StrategyId, max: StrategyId) -> bool {
    (rank(min)..=rank(max)).contains(&rank(id))
}

fn rank(id: StrategyId) -> (u8, u32) {
    match id {
        StrategyId::LowPower => (0, 0),
        StrategyId::Balanced => (1, 0),
        StrategyId::HighPerformance => (2, 0),
        StrategyId::Custom(n) => (3, n),
    }
}

/// The time on the busiest device.
pub(crate) fn busiest(time: &PerDevice<Duration>) -> Duration {
    time.cpu.max(time.gpu).max(time.npu).max(time.other)
}

/// The time moving `transfer`'s bytes takes over the links, in µs; a link of
/// rate `0` is not timed.
fn transfer_micros(transfer: &PerLink<u64>, rate: &PerLink<u64>) -> u64 {
    let over = |bytes: u64, rate: u64| {
        if rate == 0 {
            0
        } else {
            (u128::from(bytes) * 1_000_000 / u128::from(rate)) as u64
        }
    };
    over(transfer.cpu_to_gpu, rate.cpu_to_gpu) + over(transfer.gpu_to_cpu, rate.gpu_to_cpu)
}

pub(crate) fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

fn micros_of_ms(ms: f32) -> u64 {
    (f64::from(ms.max(0.0)) * 1_000.0) as u64
}

/// `values` with `f` applied on every device.
fn each<T: Copy, U: Default>(values: &PerDevice<T>, f: impl Fn(T) -> U) -> PerDevice<U> {
    PerDevice {
        cpu: f(values.cpu),
        gpu: f(values.gpu),
        npu: f(values.npu),
        other: f(values.other),
    }
}
