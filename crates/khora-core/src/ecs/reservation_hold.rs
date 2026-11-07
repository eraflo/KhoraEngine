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

//! Entity ids held for a load prepared off the frame.
//!
//! A reservation made through the [`EntityReserver`] lasts one frame: the
//! world takes back, at the frame's end, every id handed out and not
//! committed. A load prepared off the frame takes longer than that, so it
//! reserves under a [`ReservationHold`]: ids held this way are not taken back
//! at the frame's end. They stay held until the load commits them, or until
//! the load is abandoned or the hold dropped, which hands them out again.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::ecs::entity::EntityId;
use crate::ecs::EntityReserver;

/// Ids one load reserves, held across frames until it commits them or gives
/// them back. Dropping the hold gives back every id it still holds.
#[derive(Debug)]
pub struct ReservationHold {
    reserver: Arc<EntityReserver>,
    /// The ids reserved through this hold and still held by it.
    ids: Mutex<HashSet<EntityId>>,
}

impl ReservationHold {
    /// An id no other reservation and no live entity holds, held until the
    /// load commits it or the hold gives it back. Never fails.
    pub fn reserve(&self) -> EntityId {
        let id = self.reserver.reserve_held();
        self.lock().insert(id);
        id
    }

    /// Commits `id`, one this hold reserved: the hold gives it back no more,
    /// and the world brings it to life. `false` for an id it does not hold.
    pub fn commit(&self, id: EntityId) -> bool {
        if !self.lock().remove(&id) {
            return false;
        }
        self.reserver.commit_held(id)
    }

    /// Whether this hold reserves through `reserver`.
    pub fn is_over(&self, reserver: &Arc<EntityReserver>) -> bool {
        Arc::ptr_eq(&self.reserver, reserver)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<EntityId>> {
        // A panic while held leaves a list of ids: still distinct.
        self.ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for ReservationHold {
    /// Gives back every id the hold still holds: the next reservations hand
    /// them out again.
    fn drop(&mut self) {
        let ids = std::mem::take(&mut *self.lock());
        self.reserver.give_back_held(ids);
    }
}

impl EntityReserver {
    /// A hold over this reserver: ids reserved through it survive the frame's
    /// end until committed, abandoned or dropped.
    pub fn hold(self: &Arc<Self>) -> ReservationHold {
        ReservationHold {
            reserver: Arc::clone(self),
            ids: Mutex::new(HashSet::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// **A held id is not the frame's to take back.** What the world releases
    /// at a frame's end is what the reserver calls unspent: a hold's ids are
    /// never among them, an ordinary reservation's are.
    #[test]
    fn ids_reserved_under_a_hold_are_not_taken_back_as_unspent() {
        let reserver = Arc::new(EntityReserver::new());
        let hold = reserver.hold();

        let held: Vec<EntityId> = (0..4).map(|_| hold.reserve()).collect();
        let ordinary = reserver.reserve();

        let unspent = reserver.take_unspent();
        assert_eq!(unspent, vec![ordinary], "only the ordinary id is unspent");
        for id in &held {
            assert!(!unspent.contains(id), "{id:?} is held, not unspent");
        }
    }

    /// **A dropped hold gives its ids back**: the next reservations hand them
    /// out again, rather than indices past them.
    #[test]
    fn a_dropped_hold_hands_its_ids_out_again() {
        let reserver = Arc::new(EntityReserver::new());
        let hold = reserver.hold();
        let held: HashSet<EntityId> = (0..3).map(|_| hold.reserve()).collect();

        drop(hold);

        let next: HashSet<EntityId> = (0..3).map(|_| reserver.reserve()).collect();
        assert_eq!(
            next, held,
            "the dropped hold's ids were not handed out again"
        );
    }

    /// **A hold's ids are its own.** Holds, and ordinary reservations, taken
    /// from several threads at once never hand out the same id twice.
    #[test]
    fn held_and_ordinary_reservations_never_share_an_id() {
        const THREADS: usize = 4;
        const EACH: usize = 500;
        let reserver = Arc::new(EntityReserver::new());

        let taken: Vec<Vec<EntityId>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|thread| {
                    let reserver = Arc::clone(&reserver);
                    scope.spawn(move || {
                        let hold = reserver.hold();
                        let ids: Vec<EntityId> = (0..EACH)
                            .map(|i| {
                                if (thread + i) % 2 == 0 {
                                    hold.reserve()
                                } else {
                                    reserver.reserve()
                                }
                            })
                            .collect();
                        // Kept alive until every id is counted.
                        std::mem::forget(hold);
                        ids
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("a reserving thread panicked"))
                .collect()
        });

        let all: Vec<EntityId> = taken.into_iter().flatten().collect();
        let distinct: HashSet<EntityId> = all.iter().copied().collect();
        assert_eq!(distinct.len(), THREADS * EACH, "an id was handed out twice");
    }

    /// A hold travels with the load it reserves for: prepared on a worker,
    /// committed on the frame's thread.
    #[test]
    fn a_hold_can_be_sent_to_another_thread() {
        fn sendable<T: Send + 'static>() {}
        sendable::<ReservationHold>();

        let reserver = Arc::new(EntityReserver::new());
        let hold = reserver.hold();
        let (hold, id) = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    let id = hold.reserve();
                    (hold, id)
                })
                .join()
                .expect("the worker panicked")
        });
        assert!(
            !reserver.take_unspent().contains(&id),
            "an id held on a worker is still held"
        );
        drop(hold);
    }
}
