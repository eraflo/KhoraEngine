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

//! The agent's half of GORNA: the row a family runs within the envelope the
//! DCC lent it.

use std::time::Duration;

use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::Level;

use super::LaneAgent;

/// The row [`choose`] picks, and whether it is the floor run because nothing
/// fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The row, as an index into the menu.
    pub row: usize,
    /// Whether no row fitted the envelope, so the floor runs.
    pub over_budget: bool,
}

/// The time a cost takes: the busiest device's, the devices running side by
/// side.
pub(super) fn time_of(cost: &Cost) -> Duration {
    cost.time
        .cpu
        .max(cost.time.gpu)
        .max(cost.time.npu)
        .max(cost.time.other)
}

/// Whether every device's `value` is within its `limit`.
fn within<T: PartialOrd + Copy>(value: &PerDevice<T>, limit: &PerDevice<T>) -> bool {
    value.cpu <= limit.cpu
        && value.gpu <= limit.gpu
        && value.npu <= limit.npu
        && value.other <= limit.other
}

/// Whether `cost` fits `envelope`: its time and its memory, on every device.
pub(super) fn fits(cost: &Cost, envelope: &Envelope) -> bool {
    within(&cost.time, &envelope.time) && within(&cost.memory, &envelope.memory)
}

/// The row a family runs within `envelope`: among the rows of `menu` whose
/// quote fits it on every device, the least impact, then the dearest; the
/// floor (the cheapest priced row) when none fits, and `default` when no row
/// is priced. `pin` overrides the choice; `allowed` filters the rows.
pub fn choose(
    envelope: &Envelope,
    quotes: &[Option<Cost>],
    menu: &[Level],
    default: usize,
    pin: Option<StrategyId>,
    allowed: Option<&[StrategyId]>,
) -> Choice {
    if let Some(row) = pin.and_then(|pin| menu.iter().position(|level| level.id == pin)) {
        return Choice {
            row,
            over_budget: false,
        };
    }
    // The rows the policy allows that have a price.
    let priced: Vec<(usize, Cost)> = menu
        .iter()
        .enumerate()
        .filter(|(_, level)| allowed.is_none_or(|allowed| allowed.contains(&level.id)))
        .filter_map(|(row, _)| quotes.get(row).copied().flatten().map(|cost| (row, cost)))
        .collect();
    let best = priced
        .iter()
        .filter(|(_, cost)| fits(cost, envelope))
        // The least impact, then the dearest; the first row on a full tie.
        .min_by(|(a_row, a), (b_row, b)| {
            menu[*a_row]
                .impact
                .cmp(&menu[*b_row].impact)
                .then(time_of(b).cmp(&time_of(a)))
                .then(a_row.cmp(b_row))
        });
    if let Some((row, _)) = best {
        return Choice {
            row: *row,
            over_budget: false,
        };
    }
    match priced
        .iter()
        .min_by(|(a_row, a), (b_row, b)| time_of(a).cmp(&time_of(b)).then(a_row.cmp(b_row)))
    {
        Some((row, _)) => Choice {
            row: *row,
            over_budget: true,
        },
        // Nothing priced: the default row, or, when the policy does not allow
        // it, the first row it allows; a policy naming no row of the menu
        // limits nothing.
        None => {
            let allows =
                |row: &usize| allowed.is_none_or(|allowed| allowed.contains(&menu[*row].id));
            let row = Some(default)
                .filter(|row| *row < menu.len() && allows(row))
                .or_else(|| (0..menu.len()).find(allows))
                .unwrap_or(default);
            Choice {
                row,
                over_budget: false,
            }
        }
    }
}

impl LaneAgent {
    /// Chooses the row that runs within the envelope the DCC lent, from the
    /// quotes as they stand; nothing while no envelope is lent.
    pub(super) fn choose_row(&mut self) {
        let Some(lent) = &self.lent else {
            return;
        };
        // A row that failed under this envelope has no price to fit on.
        let quotes: Vec<Option<Cost>> = self
            .quotes
            .iter()
            .zip(&self.failed)
            .map(|(quote, failed)| quote.filter(|_| !failed))
            .collect();
        let choice = choose(
            &lent.envelope,
            &quotes,
            &self.levels,
            self.default_row,
            lent.pin,
            lent.allowed.as_deref(),
        );
        // Every priced row failed: the choice falls to the default row, but
        // a row that has not failed (unpriced, so not yet tried) is tried
        // before a failed one runs again; without a pin, within the policy.
        let row = if self.failed.get(choice.row).copied().unwrap_or(false) && lent.pin.is_none() {
            (0..self.menu.len())
                .find(|row| {
                    !self.failed[*row]
                        && lent
                            .allowed
                            .as_ref()
                            .is_none_or(|allowed| allowed.contains(&self.menu[*row].id))
                })
                .unwrap_or(choice.row)
        } else {
            choice.row
        };
        self.current = row;
        self.over_budget = choice.over_budget;
    }
}
