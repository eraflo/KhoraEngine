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

use std::collections::HashSet;
use std::sync::Mutex;

use crate::ecs::entity::EntityId;
use crate::sync::LockExt;

/// Entity ids a world-free caller takes, for the world that owns the reserver
/// to commit or release at its next exclusive moment.
#[derive(Debug, Default)]
pub struct EntityReserver {
    // A panic while holding the lock leaves the lists as they were: still
    // ids, still distinct.
    state: Mutex<State>,
}

impl EntityReserver {
    /// Returns a reserver owned by no world: its ids start at index 0 — what a
    /// script run on its own spawns with.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns an id no other reservation and no live entity holds. Never fails.
    pub fn reserve(&self) -> EntityId {
        let mut state = self.state.lock_recovered();
        let id = state.next();
        state.handed.push(id);
        state.served += 1;
        id
    }

    /// Returns an id like [`reserve`](Self::reserve) gives, held: never taken back
    /// at a frame's end, until committed or given back.
    pub(crate) fn reserve_held(&self) -> EntityId {
        let mut state = self.state.lock_recovered();
        let id = state.next();
        state.held.insert(id);
        id
    }

    /// Holds `ids`, slots the owning world gives a hold: never taken back
    /// at a frame's end, until committed or given back.
    pub(crate) fn hold_ids(&self, ids: &[EntityId]) {
        self.state.lock_recovered().held.extend(ids.iter().copied());
    }

    /// Returns whether `id` is held and not committed nor given back — and, if so,
    /// forgets it: the load holding it commits it.
    pub fn commit_held(&self, id: EntityId) -> bool {
        self.state.lock_recovered().held.remove(&id)
    }

    /// Gives back the held `ids` still held: handed out first by the next
    /// reservations.
    pub(crate) fn give_back_held(&self, ids: impl IntoIterator<Item = EntityId>) {
        let mut state = self.state.lock_recovered();
        for id in ids {
            if state.held.remove(&id) {
                state.ready.push(id);
            }
        }
    }

    /// Takes one id ready to hand out, for the owning world itself to fill.
    ///
    /// Spawns use the slots loads and frames left ready before growing the store.
    pub fn take_ready(&self) -> Option<EntityId> {
        self.state.lock_recovered().ready.pop()
    }

    /// Claims the next fresh index, past the end of the store, for the owning world itself.
    ///
    /// No reservation, from any thread, is handed the same index.
    pub fn claim_fresh(&self) -> u32 {
        let mut state = self.state.lock_recovered();
        let index = state.fresh;
        state.fresh += 1;
        index
    }

    /// Returns the fresh indices handed out past `from`, for the owning world to give slots to.
    ///
    /// Raises the next fresh index to `from` when it lags, so a fresh id is
    /// never an index the store already holds.
    pub fn fresh_since(&self, from: u32) -> std::ops::Range<u32> {
        let mut state = self.state.lock_recovered();
        if state.fresh < from {
            state.fresh = from;
        }
        from..state.fresh
    }

    /// Returns whether this reserver handed out `id` since the world last settled —
    /// and, if so, forgets it: a spawn commits an id once.
    pub fn take_handed(&self, id: EntityId) -> bool {
        let mut state = self.state.lock_recovered();
        match state.handed.iter().position(|handed| *handed == id) {
            Some(at) => {
                state.handed.swap_remove(at);
                true
            }
            None => false,
        }
    }

    /// Returns whether this reserver handed out `id` since the world last settled
    /// and nothing committed it yet — without forgetting it.
    pub fn is_handed(&self, id: EntityId) -> bool {
        self.state.lock_recovered().handed.contains(&id)
    }

    /// Returns every id handed out and not committed: what the world takes back.
    pub fn take_unspent(&self) -> Vec<EntityId> {
        std::mem::take(&mut self.state.lock_recovered().handed)
    }

    /// Returns how many ids are ready to hand out without a fresh index.
    pub fn ready_len(&self) -> usize {
        self.state.lock_recovered().ready.len()
    }

    /// Returns how many ids were handed out since the last call — and starts the
    /// count again.
    pub fn take_served(&self) -> usize {
        std::mem::take(&mut self.state.lock_recovered().served)
    }

    /// Takes back the ready ids past the first `keep`, for the world to free.
    pub fn take_ready_beyond(&self, keep: usize) -> Vec<EntityId> {
        let mut state = self.state.lock_recovered();
        let keep = keep.min(state.ready.len());
        state.ready.split_off(keep)
    }

    /// Gives the reserver more ids to hand out first — ids the world holds
    /// back for it.
    pub fn refill(&self, ids: impl IntoIterator<Item = EntityId>) {
        self.state.lock_recovered().ready.extend(ids);
    }
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
    /// Every id handed out under a hold and not committed or given back yet:
    /// never taken back at a frame's end.
    held: HashSet<EntityId>,
}

impl State {
    /// Returns the next id to hand out: a ready one, else a fresh index.
    fn next(&mut self) -> EntityId {
        match self.ready.pop() {
            Some(id) => id,
            None => {
                let index = self.fresh;
                self.fresh += 1;
                EntityId {
                    index,
                    generation: 0,
                }
            }
        }
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
    fn an_id_is_seen_as_handed_until_taken() {
        let reserver = EntityReserver::new();
        let handed = reserver.reserve();
        assert!(reserver.is_handed(handed));
        assert!(reserver.is_handed(handed), "looking does not forget it");
        assert!(reserver.take_handed(handed));
        assert!(!reserver.is_handed(handed));
        assert!(!reserver.is_handed(id(9, 0)));
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

    #[test]
    fn a_held_id_is_committed_once() {
        let reserver = EntityReserver::new();
        let held = reserver.reserve_held();
        assert!(!reserver.commit_held(id(9, 0)), "never held");
        assert!(reserver.commit_held(held));
        assert!(!reserver.commit_held(held), "committed already");
    }

    #[test]
    fn the_world_takes_ready_ids_and_claims_fresh_indices_no_reservation_gets() {
        let reserver = EntityReserver::new();
        reserver.refill([id(3, 2)]);
        assert_eq!(reserver.take_ready(), Some(id(3, 2)));
        assert_eq!(reserver.take_ready(), None);

        assert_eq!(reserver.claim_fresh(), 0);
        assert_eq!(reserver.claim_fresh(), 1);
        assert_eq!(reserver.reserve(), id(2, 0), "past the claimed indices");
    }
}
