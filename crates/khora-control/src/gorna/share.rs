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

//! The share: the allocation that lends a round's plans, each envelope its
//! planned cost plus 80 % of the slack, held to the row planned for it.

use std::time::Duration;

use khora_core::agent::gorna::protocol::{Allocation, Envelope, Offer, OfferRow};
use khora_core::agent::gorna::{AdaptationMode, AgentId, StrategyId};
use khora_core::lane::cost::{Cost, Device};

use super::admission::{critical_paths, held, late};
use super::plan::{within, Plan};
use super::round::{Bid, Round};
use super::units::{busiest, fits_envelope, micros, micros_of_ms};

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

/// Holds each envelope of `envelopes` to the row planned for its agent: an
/// agent runs the best row its envelope fits, so an envelope that fits a row
/// of more quality than the one planned is cut just under that row. An agent
/// holding a kept envelope keeps it.
pub(crate) fn hold_to_plan(envelopes: &mut [(AgentId, Envelope)], bids: &[&Bid], plans: &[Plan]) {
    for (bid, plan) in bids.iter().zip(plans) {
        let Some(planned) = plan.row else {
            continue;
        };
        if plan.fixed.is_some() {
            continue;
        }
        if let Some((_, envelope)) = envelopes
            .iter_mut()
            .find(|(agent, _)| *agent == bid.offer.agent)
        {
            let rows = &bid.offer.rows;
            hold(envelope, rows, &rows[planned], &rows[planned].cost);
        }
    }
}

/// Holds `allocation`, lent in the agents' quoted units, to the rows
/// `planned` at the costs the agents quoted in `quoted` — an agent chooses
/// its row by its own quotes — then says again whether a lower target can
/// take something back, `bids` giving each agent's policies. The agents of
/// `kept` keep their envelope.
pub(crate) fn hold_to_quotes(
    allocation: &mut Allocation,
    quoted: &[Offer],
    bids: &[Bid],
    planned: &[(AgentId, StrategyId)],
    kept: &[AgentId],
) {
    for (agent, row) in planned {
        if kept.contains(agent) {
            continue;
        }
        let Some(offer) = quoted.iter().find(|offer| offer.agent == *agent) else {
            continue;
        };
        let Some(quoted_row) = offer.rows.iter().find(|candidate| candidate.id == *row) else {
            continue;
        };
        if let Some((_, envelope)) = (allocation.envelopes.iter_mut()).find(|(id, _)| id == agent) {
            // The planned row still fits, as the agent quotes it.
            hold(envelope, &offer.rows, quoted_row, &quoted_row.cost);
        }
    }
    // Whether a lower target can take something back, as the agents see
    // their envelopes: by their quotes.
    let as_quoted: Vec<Bid> = (bids.iter())
        .map(|bid| {
            let mut bid = bid.clone();
            if let Some(offer) = quoted.iter().find(|offer| offer.agent == bid.offer.agent) {
                bid.offer = offer.clone();
            }
            bid
        })
        .collect();
    let refs: Vec<&Bid> = as_quoted.iter().collect();
    allocation.lever = allocation.lever && lever(&refs, &allocation.envelopes);
}

/// Whether a lower target can take something back from any agent of `bids`
/// under `envelopes`.
pub(crate) fn lever(bids: &[&Bid], envelopes: &[(AgentId, Envelope)]) -> bool {
    bids.iter().any(|bid| {
        (envelopes.iter())
            .find(|(agent, _)| *agent == bid.offer.agent)
            .is_some_and(|(_, envelope)| above_floor(bid, &Plan::of(bid), envelope))
    })
}

/// Cuts `envelope` so no row of `rows` of more quality than `planned` fits
/// it, never below `least`: each such row that fits is cut just under, on
/// the device where it costs the most above `least` by at least a
/// microsecond, else in memory. A row that costs no more than `least`
/// anywhere cannot be held off.
fn hold(envelope: &mut Envelope, rows: &[OfferRow], planned: &OfferRow, least: &Cost) {
    let step = Duration::from_micros(1);
    for better in rows.iter().filter(|row| row.quality > planned.quality) {
        if !fits_envelope(&better.cost, envelope) {
            continue;
        }
        let by_time = Device::ALL
            .into_iter()
            .filter(|device| device.get(&better.cost.time) >= device.get(&least.time) + step)
            .max_by_key(|device| device.get(&better.cost.time) - device.get(&least.time));
        if let Some(device) = by_time {
            let under = device.get(&better.cost.time) - step;
            let time = device.get_mut(&mut envelope.time);
            *time = (*time).min(under);
            continue;
        }
        let by_memory = Device::ALL
            .into_iter()
            .filter(|device| device.get(&better.cost.memory) > device.get(&least.memory))
            .max_by_key(|device| device.get(&better.cost.memory) - device.get(&least.memory));
        if let Some(device) = by_memory {
            let under = device.get(&better.cost.memory) - 1;
            let memory = device.get_mut(&mut envelope.memory);
            *memory = (*memory).min(under);
        }
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
