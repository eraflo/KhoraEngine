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

//! What a round plans for one agent — its floor, its row, what it keeps and
//! the rows its policies rule out — and how much the agent weighs.

use khora_core::agent::gorna::protocol::{CostBasis, Envelope, OfferRow};
use khora_core::agent::gorna::{AdaptationMode, StrategyId};
use khora_core::agent::AgentImportance;
use khora_core::lane::cost::{Cost, Device, PerDevice, PerLink};

use super::round::Bid;
use super::units::{busiest, each, micros, micros_of_ms};

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
    pub(super) fn of(bid: &Bid) -> Self {
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
                        within(row.id, min, max) && row.confidence != CostBasis::Assumed
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
                    || row.confidence == CostBasis::Assumed
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
    pub(super) fn asks(&self, rows: &[OfferRow]) -> (PerDevice<u64>, PerDevice<u64>, PerLink<u64>) {
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

/// How much `bid` weighs: its `Prioritize` hint, else its importance —
/// Critical 1.0, Important 0.75, Optional 0.5.
pub(crate) fn weight(bid: &Bid) -> f64 {
    f64::from(bid.hints.priority.unwrap_or(match bid.importance {
        AgentImportance::Critical => 1.0,
        AgentImportance::Important => 0.75,
        AgentImportance::Optional => 0.5,
    }))
}

/// Whether `id` lies in the `Bounded` range `min..=max`, the ids ordered
/// `LowPower < Balanced < HighPerformance < Custom(n)` by `n`.
pub(super) fn within(id: StrategyId, min: StrategyId, max: StrategyId) -> bool {
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
