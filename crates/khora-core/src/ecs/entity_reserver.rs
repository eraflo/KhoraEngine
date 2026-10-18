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

//! Entity ids handed out through a shared reference, ahead of the frame
//! boundary that brings them to life.
//!
//! A script spawns without the world: its turn runs on a worker, and what it
//! writes is applied at the frame's end. Yet `var b = Spawn(…)` must name the
//! entity at once, so a later line can write to it. The world keeps ids ready
//! for that — reserved, generation already moved on — and past them hands out
//! indices beyond the end of its store; at its next exclusive moment it gives
//! those slots to the ones handed out, commits the ids a spawn uses and takes
//! back the rest. A reservation never fails, and never names an entity that
//! lives or lived.

use std::sync::Mutex;

use crate::ecs::entity::EntityId;

/// Entity ids a world-free caller takes, for the world that owns the reserver
/// to commit or release at its next exclusive moment.
#[derive(Debug, Default)]
pub struct EntityReserver {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Ids the world holds back for this reserver, handed out first.
    ready: Vec<EntityId>,
    /// The next index past the end of the world's store to hand out.
    fresh: u32,
    /// Every id handed out since the world last settled its reservations.
    handed: Vec<EntityId>,
    /// How many ids were handed out since the world last took the count —
    /// a frame's spawns, which size the ids it keeps ready for the next.
    served: usize,
}

impl EntityReserver {
    /// A reserver owned by no world: its ids start at index 0 — what a
    /// script run on its own spawns with.
    pub fn new() -> Self {
        Self::default()
    }

    /// An id no other reservation and no live entity holds. Never fails.
    pub fn reserve(&self) -> EntityId {
        let mut state = self.lock();
        let id = match state.ready.pop() {
            Some(id) => id,
            None => {
                let index = state.fresh;
                state.fresh += 1;
                EntityId {
                    index,
                    generation: 0,
                }
            }
        };
        state.handed.push(id);
        state.served += 1;
        id
    }

    /// The fresh indices handed out up to now, past `from`: what the owning
    /// world gives slots to. Raises the next fresh index to `from` when it
    /// lags, so a fresh id is never an index the store already holds.
    pub fn fresh_since(&self, from: u32) -> std::ops::Range<u32> {
        let mut state = self.lock();
        if state.fresh < from {
            state.fresh = from;
        }
        from..state.fresh
    }

    /// Whether this reserver handed out `id` since the world last settled —
    /// and, if so, forgets it: a spawn commits an id once.
    pub fn take_handed(&self, id: EntityId) -> bool {
        let mut state = self.lock();
        match state.handed.iter().position(|handed| *handed == id) {
            Some(at) => {
                state.handed.swap_remove(at);
                true
            }
            None => false,
        }
    }

    /// Every id handed out and not committed: what the world takes back.
    pub fn take_unspent(&self) -> Vec<EntityId> {
        std::mem::take(&mut self.lock().handed)
    }

    /// How many ids are ready to hand out without a fresh index.
    pub fn ready_len(&self) -> usize {
        self.lock().ready.len()
    }

    /// How many ids were handed out since the last call — and starts the
    /// count again.
    pub fn take_served(&self) -> usize {
        std::mem::take(&mut self.lock().served)
    }

    /// Takes back the ready ids past the first `keep`, for the world to free.
    pub fn take_ready_beyond(&self, keep: usize) -> Vec<EntityId> {
        let mut state = self.lock();
        let keep = keep.min(state.ready.len());
        state.ready.split_off(keep)
    }

    /// Gives the reserver more ids to hand out first — ids the world holds
    /// back for it.
    pub fn refill(&self, ids: impl IntoIterator<Item = EntityId>) {
        self.lock().ready.extend(ids);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock leaves the lists as they were: still
        // ids, still distinct.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(index: u32, generation: u32) -> EntityId {
        EntityId { index, generation }
    }

    #[test]
    fn ready_ids_are_handed_out_before_fresh_indices() {
        let reserver = EntityReserver::new();
        reserver.refill([id(3, 2)]);
        assert_eq!(reserver.reserve(), id(3, 2));
        assert_eq!(reserver.reserve(), id(0, 0));
    }

    #[test]
    fn fresh_indices_start_past_the_store() {
        let reserver = EntityReserver::new();
        assert_eq!(reserver.fresh_since(5), 5..5);
        assert_eq!(reserver.reserve(), id(5, 0));
        assert_eq!(reserver.fresh_since(5), 5..6);
    }

    #[test]
    fn an_id_is_taken_as_handed_once() {
        let reserver = EntityReserver::new();
        let handed = reserver.reserve();
        assert!(!reserver.take_handed(id(9, 0)));
        assert!(reserver.take_handed(handed));
        assert!(!reserver.take_handed(handed));
        assert!(reserver.take_unspent().is_empty());
    }

    #[test]
    fn the_served_count_starts_again_once_taken() {
        let reserver = EntityReserver::new();
        reserver.reserve();
        reserver.reserve();
        assert_eq!(reserver.take_served(), 2);
        assert_eq!(reserver.take_served(), 0);
    }

    #[test]
    fn ready_ids_past_those_kept_are_taken_back() {
        let reserver = EntityReserver::new();
        reserver.refill([id(1, 1), id(2, 1), id(3, 1)]);
        assert_eq!(reserver.take_ready_beyond(1), vec![id(2, 1), id(3, 1)]);
        assert_eq!(reserver.ready_len(), 1);
        assert!(reserver.take_ready_beyond(5).is_empty());
    }
}
