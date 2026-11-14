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

//! The problem a round plans beyond its own frame, which every proposer and
//! the shield plan and check.
//!
//! See the book: concepts/gorna.md, "Formalisation".

use std::time::Duration;

use khora_core::lane::cost::Device;

use super::super::decide::{critical_paths, weight, Bid, Plan, Round};

/// The problem `round` plans, its bids (in the order of `bids`) grown as
/// the horizon expects.
pub(crate) struct Problem {
    /// The bids, each row above its agent's floor grown.
    pub(crate) bids: Vec<Bid>,
    /// The target the plans share.
    pub(crate) target: Duration,
}

impl Problem {
    /// Returns the problem `round` plans for `bids`, whose base plans are `base`.
    ///
    /// Rows above the floor, other than the base row, are grown by the agent's
    /// expected growth. The plans share the target less the margin, never less
    /// than the base plans take on the critical path: growth and margin weigh
    /// the upgrades, never refuse what admission let in.
    pub(crate) fn of(round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> Self {
        let bids: Vec<Bid> = (bids.iter().zip(base))
            .map(|(bid, plan)| grown(round, bid, plan.row))
            .collect();
        let refs: Vec<&Bid> = bids.iter().collect();
        let paths = critical_paths(round, &refs, base);
        let base_path = Device::ALL
            .into_iter()
            .map(|device| device.get(&paths))
            .max()
            .unwrap_or_default();
        let target = round
            .target
            .saturating_sub(round.planning.margin)
            .max(Duration::from_micros(base_path));
        Self { bids, target }
    }

    /// Returns the bids, by reference, in their order.
    pub(crate) fn refs(&self) -> Vec<&Bid> {
        self.bids.iter().collect()
    }

    /// Returns `round` as this problem plans it: its bids, and its target.
    pub(crate) fn round<'s>(&'s self, round: &Round<'s>) -> Round<'s> {
        Round {
            target: self.target,
            bids: &self.bids,
            ..round.clone()
        }
    }
}

/// Returns `bid` with each row above its floor, but its base row `base`,
/// grown by its agent's expected growth when that growth is above 1.
fn grown(round: &Round<'_>, bid: &Bid, base: Option<usize>) -> Bid {
    let mut bid = bid.clone();
    let growth = round.planning.growth_of(bid.offer.agent);
    if !growth.is_finite() || growth <= 1.0 {
        return bid;
    }
    let floor = bid.offer.floor;
    let scale = |time: Duration| Duration::from_secs_f64(time.as_secs_f64() * growth);
    for (_, row) in (bid.offer.rows.iter_mut().enumerate())
        .filter(|(index, row)| row.id != floor && Some(*index) != base)
    {
        let time = &mut row.cost.time;
        time.cpu = scale(time.cpu);
        time.gpu = scale(time.gpu);
        time.npu = scale(time.npu);
        time.other = scale(time.other);
    }
    bid
}

/// Returns what switching `bid`'s agent to `row` charges, in weighed quality.
///
/// Its weight times the switch's preparation and transient, over the
/// horizon. The running row, a row the planning names no preparation for,
/// and a round with no horizon charge nothing.
pub(crate) fn charge(round: &Round<'_>, bid: &Bid, row: usize) -> f64 {
    let planning = &round.planning;
    let Some(offered) = bid.offer.rows.get(row) else {
        return 0.0;
    };
    if planning.horizon.is_zero() || offered.id == bid.offer.current {
        return 0.0;
    }
    match planning.switch_of(bid.offer.agent, offered.id) {
        Some(prepare) => {
            weight(bid) * (prepare + planning.transient).as_secs_f64()
                / planning.horizon.as_secs_f64()
        }
        None => 0.0,
    }
}

/// Returns what planning `row` for `bid` is worth: its weighed quality, less
/// what switching to it charges. Nothing planned is worth nothing.
pub(crate) fn value_of(round: &Round<'_>, bid: &Bid, row: Option<usize>) -> f64 {
    row.map_or(0.0, |row| {
        weight(bid) * f64::from(bid.offer.rows[row].quality) - charge(round, bid, row)
    })
}

/// Returns what `plans` are worth for `bids`: each one's planned row's value.
pub(crate) fn value(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> f64 {
    bids.iter()
        .zip(plans)
        .map(|(bid, plan)| value_of(round, bid, plan.row))
        .sum()
}
