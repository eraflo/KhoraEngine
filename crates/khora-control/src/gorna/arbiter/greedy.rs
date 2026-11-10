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

//! The Q-RAM pass as a proposer.

use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::protocol::{Binding, DecidedBy, Rejected};
use khora_core::lane::cost::{Cost, Device};

use super::super::decide::{breach, busiest, fits_envelope, micros, weight, Bid, Plan, Round};
use super::{Proposal, Proposer};

/// Upgrades by weighed quality gained per microsecond, climbing each agent's
/// convex hull of priced rows while they fit.
pub(crate) struct Greedy;

impl Proposer for Greedy {
    fn name(&self) -> DecidedBy {
        DecidedBy::Greedy
    }

    /// While an upgrade fits, take the one that gains the most weighed
    /// quality per microsecond, climbing each agent's convex hull of priced
    /// rows. An upgrade that does not fit is rejected, with the constraint
    /// it broke; a smaller one of the same agent may still fit. The price is
    /// the gain per µs of the last upgrade taken.
    fn propose(&self, round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> Proposal {
        let mut plans = base.to_vec();
        let mut price = None;
        let mut rejected = Vec::new();
        loop {
            let mut best: Option<Step> = None;
            for (agent, (bid, plan)) in bids.iter().zip(plans.iter()).enumerate() {
                if plan.fixed.is_some() {
                    continue;
                }
                if let Some(step) = next_step(agent, bid, plan) {
                    if best
                        .as_ref()
                        .is_none_or(|best| step.goes_before(best, bids))
                    {
                        best = Some(step);
                    }
                }
            }
            let Some(step) = best else {
                break;
            };
            let before = plans[step.agent].row;
            plans[step.agent].row = Some(step.row);
            let offered = &bids[step.agent].offer.rows[step.row];
            let over = match plans[step.agent].ceiling {
                Some(ceiling) if !fits_envelope(&offered.cost, &ceiling) => {
                    Some(over_ceiling(&offered.cost, &ceiling))
                }
                _ => breach(round, bids, &plans),
            };
            match over {
                None => price = Some(step.slope as f32),
                Some(over) => {
                    // This step does not fit; a smaller one of the same agent
                    // may.
                    plans[step.agent].row = before;
                    plans[step.agent].out[step.row] = true;
                    rejected.push(Rejected {
                        agent: bids[step.agent].offer.agent,
                        row: offered.id,
                        over,
                    });
                }
            }
        }
        Proposal {
            rows: plans.iter().map(|plan| plan.row).collect(),
            price,
            rejected,
        }
    }
}

/// What a `Stable` ceiling refuses of `cost`: the ceiling on the first
/// device whose time or memory it is over.
fn over_ceiling(cost: &Cost, ceiling: &Envelope) -> Binding {
    let device = Device::ALL
        .into_iter()
        .find(|device| {
            device.get(&cost.time) > device.get(&ceiling.time)
                || device.get(&cost.memory) > device.get(&ceiling.memory)
        })
        .unwrap_or(Device::Cpu);
    Binding::Ceiling(device)
}

/// One candidate upgrade: an agent, the row it moves to, and what it gains.
struct Step {
    agent: usize,
    row: usize,
    /// Weighed quality gained per microsecond.
    slope: f64,
    /// Weighed quality gained.
    gain: f64,
}

impl Step {
    /// Whether `self` is taken before `other`: the steeper gain, then the
    /// larger gain, then the agent named first, then the lower row.
    fn goes_before(&self, other: &Step, bids: &[&Bid]) -> bool {
        self.slope
            .total_cmp(&other.slope)
            .then(self.gain.total_cmp(&other.gain))
            .then(
                bids[other.agent]
                    .offer
                    .agent
                    .cmp(&bids[self.agent].offer.agent),
            )
            .then(other.row.cmp(&self.row))
            .is_gt()
    }
}

/// The next step up `bid`'s convex hull from the row `plan` holds: among the
/// rows not ruled out, of greater quality and no lesser cost, the steepest
/// gain per microsecond (the larger gain, then the lower row, on a tie).
fn next_step(agent: usize, bid: &Bid, plan: &Plan) -> Option<Step> {
    let rows = &bid.offer.rows;
    let weight = weight(bid);
    let (cost_now, quality_now) = match plan.row {
        Some(row) => (micros(busiest(&rows[row].cost.time)), rows[row].quality),
        None => (micros(busiest(&plan.floor.time)), 0.0),
    };
    rows.iter()
        .enumerate()
        .filter(|(row, candidate)| {
            !plan.out[*row]
                && candidate.quality > quality_now
                && micros(busiest(&candidate.cost.time)) >= cost_now
        })
        .map(|(row, candidate)| {
            let gain = weight * f64::from(candidate.quality - quality_now);
            let extra = micros(busiest(&candidate.cost.time)) - cost_now;
            let slope = if extra == 0 {
                f64::INFINITY
            } else {
                gain / extra as f64
            };
            Step {
                agent,
                row,
                slope,
                gain,
            }
        })
        .reduce(|kept, next| {
            let next_first = next
                .slope
                .total_cmp(&kept.slope)
                .then(next.gain.total_cmp(&kept.gain))
                .then(kept.row.cmp(&next.row))
                .is_gt();
            if next_first {
                next
            } else {
                kept
            }
        })
}
