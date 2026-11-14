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
use khora_core::lane::level::{Impact, Level};

use super::{LaneAgent, Lent};

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

/// How much running `level` on the inputs its `quote` priced changes the
/// result: what the quote names, else the level's own impact.
pub(super) fn impact_of(quote: Option<&Cost>, level: &Level) -> Impact {
    quote.and_then(|cost| cost.impact).unwrap_or(level.impact)
}

/// What running `level` on the inputs its `quote` priced is worth: the
/// quality of its impact times the share of the work it serves.
pub(super) fn quality_of(quote: Option<&Cost>, level: &Level) -> f32 {
    impact_of(quote, level).quality() * quote.map_or(1.0, Cost::served)
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

/// Whether the agent never runs `row` while another lane's row stands: one of
/// `others` — `(row, lane, quote, quality)` — in another lane, of no worse
/// quality, no dearer on any device, in time or memory, and better on one of
/// them or listed first. [`choose`] takes the cheapest lane between equals,
/// so the dominating row fits wherever `row` does and is taken instead.
pub(super) fn dominated(
    row: (usize, usize, &Cost, f32),
    others: &[(usize, usize, Cost, f32)],
) -> bool {
    let (index, lane, cost, quality) = row;
    others
        .iter()
        .any(|(other, other_lane, other_cost, other_quality)| {
            *other_lane != lane
                && *other_quality >= quality
                && within(&other_cost.time, &cost.time)
                && within(&other_cost.memory, &cost.memory)
                && (*other_quality > quality
                    || other_cost.time != cost.time
                    || other_cost.memory != cost.memory
                    || *other < index)
        })
}

/// The row a family runs within `envelope`: among the rows of `menu` whose
/// quote fits it on every device, the best quality — the quality of the
/// impact (the quote's, else the level's) times the share of the work the
/// quote serves; between equals, the dearest level of each lane, then the
/// cheapest of those lanes. The floor (the cheapest priced row) when none
/// fits, and `default` when no row is priced. `pin` overrides the choice;
/// `allowed` filters the rows.
///
/// `lanes[row]` is the lane `row` belongs to: a lane's rows are its levels,
/// cheapest first.
pub fn choose(
    envelope: &Envelope,
    quotes: &[Option<Cost>],
    menu: &[Level],
    lanes: &[usize],
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
    // A row its menu names no lane for stands alone.
    let lane_of = |row: usize| lanes.get(row).map_or(usize::MAX - row, |lane| *lane);
    let fitting: Vec<(usize, Cost, f32)> = priced
        .iter()
        .filter(|(_, cost)| fits(cost, envelope))
        .map(|(row, cost)| (*row, *cost, quality_of(Some(cost), &menu[*row])))
        .collect();
    // The best quality first. Between equals, each lane's dearest level — more
    // of the same work — then the cheapest of those lanes, which give the same
    // result; the first row on a full tie.
    let best = fitting
        .iter()
        .map(|(_, _, quality)| *quality)
        .max_by(f32::total_cmp);
    let mut per_lane: Vec<(usize, Duration)> = Vec::new();
    for (row, cost, quality) in &fitting {
        if Some(*quality) != best {
            continue;
        }
        let time = time_of(cost);
        match per_lane
            .iter_mut()
            .find(|(kept, _)| lane_of(*kept) == lane_of(*row))
        {
            Some(kept) if time > kept.1 => *kept = (*row, time),
            Some(_) => {}
            None => per_lane.push((*row, time)),
        }
    }
    let cheapest = per_lane
        .iter()
        .min_by(|(a_row, a), (b_row, b)| a.cmp(b).then(a_row.cmp(b_row)));
    if let Some((row, _)) = cheapest {
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
    /// The row that runs on `lent` with no envelope — the round could not
    /// price the agent: the pinned row; else the running row while the policy
    /// allows it and it has not failed; else the default row, then the first,
    /// that the policy allows and that has not failed; else, every allowed
    /// row failed, the running row.
    fn kept_to_policy(&self, lent: &Lent) -> usize {
        if let Some(row) = (lent.pin).and_then(|pin| self.menu.iter().position(|row| row.id == pin))
        {
            return row;
        }
        let usable = |row: usize| {
            !self.failed.get(row).copied().unwrap_or(false)
                && (lent.allowed.as_ref())
                    .is_none_or(|allowed| allowed.contains(&self.menu[row].id))
        };
        [self.current, self.default_row]
            .into_iter()
            .chain(0..self.menu.len())
            .find(|row| *row < self.menu.len() && usable(*row))
            .unwrap_or(self.current)
    }

    /// Chooses the row that runs within the envelope the DCC lent, from the
    /// quotes as they stand; nothing while nothing is lent. Lent a pin or
    /// allowed rows but no envelope, it keeps to them
    /// ([`kept_to_policy`](Self::kept_to_policy)).
    pub(super) fn choose_row(&mut self) {
        let Some(lent) = &self.lent else {
            return;
        };
        let Some(envelope) = &lent.envelope else {
            self.current = self.kept_to_policy(lent);
            self.over_budget = false;
            return;
        };
        // A row that failed under this envelope has no price to fit on.
        let quotes: Vec<Option<Cost>> = self
            .quotes
            .iter()
            .zip(&self.failed)
            .map(|(quote, failed)| quote.filter(|_| !failed))
            .collect();
        let lanes: Vec<usize> = self.menu.iter().map(|row| row.lane).collect();
        let choice = choose(
            envelope,
            &quotes,
            &self.levels,
            &lanes,
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
