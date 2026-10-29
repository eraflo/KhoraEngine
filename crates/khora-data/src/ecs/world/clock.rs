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
}

impl Default for WorldClock {
    fn default() -> Self {
        Self {
            // 0 is "never ran": a first run at tick 0 could not be told
            // apart from an observer that never looked.
            tick: Tick(1),
            observed: std::collections::HashSet::new(),
            last_seen: std::collections::HashMap::new(),
        }
    }
}

impl World {
    /// Starts a run on this world's clock and returns its tick: everything
    /// the run writes carries it. The substrate starts one run per
    /// `DataSystem`, per `Flow` and per scheduler wave.
    pub fn begin_run(&mut self) -> Tick {
        self.clock.tick
    }

    /// Ends the run in progress: the clock moves past its tick, so whatever
    /// is written next — by the next run, or between runs (the app's update,
    /// an editor edit) — carries a tick no observer has seen yet.
    pub fn end_run(&mut self) {
        self.clock.tick = Tick(self.clock.tick.0 + 1);
    }

    /// The tick a write made now carries. It is always strictly greater than
    /// every observer's last-seen tick; a new world starts at `Tick(1)`, so
    /// `Tick(0)` means "never ran".
    pub fn current_tick(&self) -> Tick {
        self.clock.tick
    }

    /// Records `key` as observed: some observer watches its changes. Called
    /// for every key a registration `observes`, and at run time for a
    /// declared (Ergon) component.
    pub fn observe(&mut self, key: ComponentKey) {
        self.clock.observed.insert(key);
    }

    /// Whether `key` is observed on this world.
    pub fn is_observed(&self, key: ComponentKey) -> bool {
        self.clock.observed.contains(&key)
    }

    /// The tick at which `observer` last ran on this world — what it has seen
    /// of its changes; `Tick(0)` if it never ran here. The ledger lives in the
    /// world, so a new world restarts every observer.
    pub fn observer_last_seen(&self, observer: Observer) -> Tick {
        self.clock
            .last_seen
            .get(&observer)
            .copied()
            .unwrap_or_default()
    }

    /// Leaves a row of page `page_id` behind: no live entity reads it any
    /// more. The page's live rows changed, so its order version moves; the
    /// row waits for compaction.
    pub(crate) fn orphan_row(&mut self, page_id: u32) {
        if let Some(page) = self.storage.pages.get_mut(page_id as usize) {
            page.order += 1;
        }
        self.storage.dirty_pages.insert(page_id);
    }

    /// Records that `observer` ran on this world at `tick`.
    pub fn note_observer_run(&mut self, observer: Observer, tick: Tick) {
        self.clock.last_seen.insert(observer, tick);
    }

    /// The tick the row of `key` on `entity` is known to have been written by
    /// at the latest (`max(τ, φ)`); `Tick(0)` when it has none.
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
