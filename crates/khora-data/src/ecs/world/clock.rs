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

//! The world's clock: the tick a write carries, the components observers
//! watch, each observer's last-seen tick, and the moves of a page's rows.

use khora_core::ecs::entity::EntityId;

use super::World;
use crate::ecs::{ComponentKey, Observer, Tick};

/// What the world knows of time and of who watches it.
#[derive(Debug)]
pub(super) struct WorldClock {
    /// The tick of the run in progress, or of the last one.
    pub(super) tick: Tick,
    /// The component keys some observer watches.
    pub(super) observed: std::collections::HashSet<ComponentKey>,
    /// Each observer's last-seen tick on this world.
    pub(super) last_seen: std::collections::HashMap<Observer, Tick>,
    /// The observers skipped since they last ran, whose next run resumes.
    ///
    /// Their last-seen tick stays, so that run sees what changed meanwhile.
    pub(super) skipped: std::collections::HashSet<Observer>,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self {
            // 0 is "never ran": a first run at tick 0 could not be told
            // apart from an observer that never looked.
            tick: Tick(1),
            observed: std::collections::HashSet::new(),
            last_seen: std::collections::HashMap::new(),
            skipped: std::collections::HashSet::new(),
        }
    }
}

impl World {
    /// Starts a run on this world's clock and returns its tick, which its writes carry.
    ///
    /// See the book: concepts/change-detection.md, "The clock".
    pub fn begin_run(&mut self) -> Tick {
        self.clock.tick
    }

    /// Ends the run in progress, moving the clock past its tick.
    ///
    /// Whatever is written next, in a run or between runs, carries a tick no
    /// observer has seen yet.
    pub fn end_run(&mut self) {
        self.clock.tick = Tick(self.clock.tick.0 + 1);
    }

    /// Returns the tick a write made now carries.
    ///
    /// Strictly greater than every observer's last-seen tick; a new world
    /// starts at `Tick(1)`, so `Tick(0)` means "never ran".
    pub fn current_tick(&self) -> Tick {
        self.clock.tick
    }

    /// Records `key` as observed: some observer watches its changes.
    pub fn observe(&mut self, key: ComponentKey) {
        self.clock.observed.insert(key);
    }

    /// Returns whether `key` is observed on this world.
    pub fn is_observed(&self, key: ComponentKey) -> bool {
        self.clock.observed.contains(&key)
    }

    /// Returns the tick at which `observer` last ran on this world, or `Tick(0)`.
    ///
    /// The ledger lives in the world, so a new world restarts every observer.
    pub fn observer_last_seen(&self, observer: Observer) -> Tick {
        self.clock
            .last_seen
            .get(&observer)
            .copied()
            .unwrap_or_default()
    }

    /// Leaves a row of page `page_id` behind for compaction, moving the page's order version.
    pub(crate) fn orphan_row(&mut self, page_id: u32) {
        if let Some(page) = self.storage.pages.get_mut(page_id as usize) {
            page.order += 1;
        }
        self.storage.dirty_pages.insert(page_id);
        self.note_page_changed(page_id);
    }

    /// Raises to now the epoch of every domain page `page_id` holds columns of.
    ///
    /// Called wherever a row is pushed onto the page or its order ω moves.
    pub(crate) fn note_page_changed(&mut self, page_id: u32) {
        let Some(page) = self.storage.pages.get(page_id as usize) else {
            return;
        };
        let now = self.clock.tick.0;
        for key in &page.keys {
            if let Some(domain) = self.storage.registry.domain_of(*key) {
                let epoch = &mut self.domain_epochs[domain.index()];
                *epoch = (*epoch).max(now);
            }
        }
    }

    /// Returns a pointer to `domain`'s change epoch in `world`, for a writable guard to raise.
    ///
    /// # Safety
    ///
    /// `world` is valid and keeps write permission over the world for as
    /// long as the pointer is used; no reference to the epochs is live when
    /// it is written through.
    pub(crate) unsafe fn domain_epoch_slot(
        world: *mut World,
        domain: crate::ecs::SemanticDomain,
    ) -> *mut u64 {
        // A place projection: no reference to the world or its epochs is made.
        unsafe { std::ptr::addr_of_mut!((*world).domain_epochs[domain.index()]) }
    }

    /// Raises the change epoch of `key`'s domain to now.
    fn note_written(&mut self, key: ComponentKey) {
        if let Some(domain) = self.storage.registry.domain_of(key) {
            self.bump_domain_epoch(domain);
        }
    }

    /// Notes a write of row `row` of `key` in page `page_id`, now.
    ///
    /// An observed column without row ticks gains them first, so the write is
    /// known for that row alone.
    pub(crate) fn stamp_row(&mut self, page_id: u32, key: ComponentKey, row: usize) {
        let observed = self.clock.observed.contains(&key);
        let tick = self.clock.tick;
        if let Some(page) = self.storage.pages.get_mut(page_id as usize) {
            if observed {
                page.materialize_row_ticks(key);
            }
            if let Some(ticks) = page.ticks.get_mut(&key) {
                ticks.stamp(row, tick);
            }
        }
        self.note_written(key);
    }

    /// Notes a write of every row of `key` in page `page_id`, now.
    pub(crate) fn stamp_column(&mut self, page_id: u32, key: ComponentKey) {
        let observed = self.clock.observed.contains(&key);
        let tick = self.clock.tick;
        if let Some(page) = self.storage.pages.get_mut(page_id as usize) {
            if observed {
                page.materialize_row_ticks(key);
            }
            if let Some(ticks) = page.ticks.get_mut(&key) {
                ticks.stamp_all(tick);
            }
        }
        self.note_written(key);
    }

    /// Drops the row ticks of every quiet column every observer has seen; returns how many.
    ///
    /// A column qualifies when `current_tick − σ ≥ quiet` and `last_seen ≥ σ`
    /// for every observer that is not skipped. Its floor rises to σ; no page's
    /// order version moves. See the book: concepts/change-detection.md,
    /// "Forgetting quietly".
    pub fn free_quiet_row_ticks(&mut self, quiet: u64) -> usize {
        let now = self.clock.tick;
        // A skipped observer holds nothing back: freed rows read as written
        // at the column's floor, which it has not seen, so its resuming run
        // still sees every one of them changed.
        let seen = self
            .clock
            .last_seen
            .iter()
            .filter(|(observer, _)| !self.clock.skipped.contains(observer))
            .map(|(_, tick)| *tick)
            .min();
        let mut freed = 0;
        for page in &mut self.storage.pages {
            for ticks in page.ticks.values_mut() {
                let quiet_enough = now.0.saturating_sub(ticks.summary.0) >= quiet;
                let seen_by_all = seen.is_none_or(|seen| seen >= ticks.summary);
                if ticks.rows.is_some() && quiet_enough && seen_by_all {
                    ticks.free_rows();
                    freed += 1;
                }
            }
        }
        freed
    }

    /// Records that `observer` ran on this world at `tick`.
    pub fn note_observer_run(&mut self, observer: Observer, tick: Tick) {
        self.clock.last_seen.insert(observer, tick);
        self.clock.skipped.remove(&observer);
    }

    /// Records that `observer` was due and did not run; its last-seen tick stays.
    pub fn note_observer_skip(&mut self, observer: Observer) {
        self.clock.skipped.insert(observer);
    }

    /// Returns whether `observer`'s next run here resumes: it never ran, or was skipped since.
    pub fn observer_resumes(&self, observer: Observer) -> bool {
        !self.clock.last_seen.contains_key(&observer) || self.clock.skipped.contains(&observer)
    }

    /// Returns the latest tick the row of `key` on `entity` is known written by (`max(τ, φ)`).
    ///
    /// `Tick(0)` when it has none.
    pub(crate) fn carried_tick(&self, entity: EntityId, key: ComponentKey) -> Tick {
        let Some(domain) = self.storage.registry.domain_of(key) else {
            return Tick::default();
        };
        self.entities
            .get(entity.index as usize)
            .and_then(|(id, metadata)| {
                if *id == entity {
                    metadata.as_ref()
                } else {
                    None
                }
            })
            .and_then(|metadata| metadata.locations.get(&domain).copied())
            .and_then(|loc| {
                self.storage
                    .pages
                    .get(loc.page_id as usize)
                    .and_then(|page| page.ticks.get(&key))
                    .map(|ticks| ticks.carried(loc.row_index as usize))
            })
            .unwrap_or_default()
    }
}
