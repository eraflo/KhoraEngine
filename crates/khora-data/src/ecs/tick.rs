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

//! The world's clock.

/// A moment of a [`World`](crate::ecs::World)'s clock: one tick per run — of
/// a `DataSystem`, of a `Flow`, of a scheduler wave. A `u64` that never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u64);

/// The silence, in ticks, after which `ecs_maintenance` frees a column's row
/// ticks ([`World::free_quiet_row_ticks`](crate::ecs::World::free_quiet_row_ticks)):
/// about five seconds of frames at 60 frames a second — a frame of the
/// sandbox scene runs about 41 ticks (one per `DataSystem`, `Flow` and
/// scheduler wave). A count of ticks, not of seconds: a column is freed once
/// that much of the world's work went by without writing it.
pub const QUIET_ROW_TICKS: u64 = 12_300;

/// The ticks a page keeps beside one of its columns, under the column's own
/// [`ComponentKey`](crate::ecs::ComponentKey): what was written to it, and
/// when.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnTicks {
    /// σ — the latest tick a row of the column was written at.
    pub summary: Tick,
    /// φ — the tick at which the column's row history was dropped: every row
    /// counts as written at least this late.
    pub floor: Tick,
    /// τ — one tick per row, in row order, for a column some observer
    /// watches while it moves; `None` otherwise (every row then reads 0).
    pub rows: Option<Vec<u64>>,
}

impl ColumnTicks {
    /// The tick a row is known to have been written by at the latest:
    /// `max(τ, φ)` — exact while the row holds its own tick, the floor once
    /// its history was dropped. What a row carries when it moves.
    pub fn carried(&self, row: usize) -> Tick {
        let own = self
            .rows
            .as_ref()
            .and_then(|rows| rows.get(row))
            .copied()
            .unwrap_or(0);
        Tick(own.max(self.floor.0))
    }

    /// Notes a row pushed at the end of the column, written at `tick`. An
    /// observed column keeps one tick per row — materialising the array, the
    /// rows before reading 0 under the floor — an unobserved one only its
    /// summary and its floor, which then follows the summary so that every
    /// row stays covered.
    pub(crate) fn push(&mut self, tick: Tick, rows_before: usize, observed: bool) {
        match (&mut self.rows, observed) {
            (Some(rows), _) => rows.push(tick.0),
            (None, true) => {
                let mut rows = vec![0; rows_before];
                rows.push(tick.0);
                self.rows = Some(rows);
            }
            (None, false) => self.floor = self.floor.max(tick),
        }
        self.summary = self.summary.max(tick);
    }

    /// Notes a write of `row` at `tick`: the row's own tick when the column
    /// keeps them, the floor otherwise, so that the row stays covered; the
    /// summary in both cases.
    pub(crate) fn stamp(&mut self, row: usize, tick: Tick) {
        match self.rows.as_mut().and_then(|rows| rows.get_mut(row)) {
            Some(own) => *own = (*own).max(tick.0),
            None => self.floor = self.floor.max(tick),
        }
        self.summary = self.summary.max(tick);
    }

    /// Notes a write of every row at `tick`.
    pub(crate) fn stamp_all(&mut self, tick: Tick) {
        match &mut self.rows {
            Some(rows) => rows.iter_mut().for_each(|own| *own = (*own).max(tick.0)),
            None => self.floor = self.floor.max(tick),
        }
        self.summary = self.summary.max(tick);
    }

    /// Whether `row` may have been written after `since`: never when the
    /// column's summary is not past it; by its own tick when the column keeps
    /// row ticks and `since` is at or above the floor; otherwise yes — the
    /// row's history was dropped above `since`, so it is not known.
    pub fn changed_since(&self, row: usize, since: Tick) -> bool {
        if self.summary <= since {
            return false;
        }
        match &self.rows {
            Some(rows) if since >= self.floor => rows.get(row).is_none_or(|&own| own > since.0),
            _ => true,
        }
    }

    /// Gives a column without row ticks one per row, each reading 0 — under
    /// the floor, which still covers them. A column that has them keeps them.
    pub(crate) fn materialize(&mut self, row_count: usize) {
        if self.rows.is_none() {
            self.rows = Some(vec![0; row_count]);
        }
    }

    /// Drops the column's row ticks: its rows read 0 under a floor raised to
    /// the summary, which still covers every one of them (I2).
    pub(crate) fn free_rows(&mut self) {
        self.rows = None;
        self.floor = self.floor.max(self.summary);
    }

    /// Removes `row` by moving the last row into its place, as the column
    /// does.
    pub(crate) fn swap_remove(&mut self, row: usize) {
        if let Some(rows) = &mut self.rows {
            if row < rows.len() {
                rows.swap_remove(row);
            }
        }
    }
}

/// Someone who watches the world's changes: a `DataSystem` or a `Flow`, by
/// its registered name. Both kinds may share a name, so the kind is part of
/// who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Observer {
    /// A `DataSystem`, by its `DataSystemRegistration::name`.
    DataSystem(&'static str),
    /// A `Flow`, by its `FlowRegistration::name`.
    Flow(&'static str),
}
