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

//! What the engine has for the scripting agent, waiting for the next frame.
//!
//! Two things cross into scripting from outside it: a module the hot-reload
//! pump recompiled, and an event some other subsystem raised. Both wait in a
//! queue the producer writes and the agent drains, and both used to reach the
//! agent through a `Flow` that read nothing from the `World` — the only door
//! open to an agent that could not declare what it touched.
//!
//! It can declare it now ([`Contention`](crate::agent::Contention)), so the
//! queue is reached directly and the two flows are gone.
//!
//! # Delivery is next frame, on purpose
//!
//! Nothing here is handled in the frame it was produced. An event handled where
//! it was raised opens a cascade with no bound, and a budget that cannot bound
//! the work it pays for is not a budget.

use std::fmt;
use std::sync::{Arc, Mutex};

/// Whether a newly queued item makes an already-queued one obsolete.
///
/// The distinction between the two queues lives here rather than in two queue
/// types, because it is a fact about the *item*: two saves of the same file
/// between frames are one reload, and two collisions in one frame are two
/// events. Nothing about a queue decides that.
pub trait Supersedes {
    /// Whether `self`, arriving now, replaces `earlier` in place.
    ///
    /// The default is never — appending is what a queue does unless the item
    /// has a reason to say otherwise.
    fn supersedes(&self, earlier: &Self) -> bool {
        let _ = earlier;
        false
    }
}

/// A queue a producer fills and the scripting agent drains.
///
/// Shared, because the two ends are a service and an agent and neither owns the
/// other. The agent declares it with
/// [`Contention::locking`](crate::agent::Contention::locking) and reaches it
/// through [`EngineContext::locked`](crate::EngineContext::locked), so the
/// scheduler knows about the lock before it groups the wave rather than
/// measuring it afterwards.
pub struct Pending<T> {
    queue: Arc<Mutex<Vec<T>>>,
}

// Written out rather than derived: a derive would demand `T: Clone` and
// `T: Default`, which an `Arc` handle never needs — cloning shares the queue,
// it does not copy what is in it.
impl<T> Clone for Pending<T> {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
        }
    }
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl<T: Supersedes> Pending<T> {
    /// An empty queue.
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queues an item, replacing whatever it supersedes.
    pub fn push(&self, item: T) {
        let Ok(mut queue) = self.queue.lock() else {
            log::error!(
                "the pending {} queue is poisoned; dropping an item",
                std::any::type_name::<T>()
            );
            return;
        };
        match queue.iter_mut().find(|earlier| item.supersedes(earlier)) {
            Some(earlier) => *earlier = item,
            None => queue.push(item),
        }
    }

    /// Takes everything queued.
    ///
    /// Drained rather than read: each of these applies once. Leaving a reload
    /// queued would re-apply it every frame, resetting an instance's fields to
    /// what the file says instead of what the game has since made of them; and
    /// leaving a `Damaged` event queued is an entity that never stops taking
    /// the same hit.
    pub fn drain(&self) -> Vec<T> {
        match self.queue.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => Vec::new(),
        }
    }

    /// How many are waiting.
    pub fn len(&self) -> usize {
        self.queue.lock().map(|queue| queue.len()).unwrap_or(0)
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> fmt::Debug for Pending<T> {
    /// Reports the depth without locking to print the contents — a `Debug` that
    /// takes the same lock the producer holds turns a log line into a stall.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let depth = self.queue.try_lock().map(|queue| queue.len());
        match depth {
            Ok(depth) => write!(f, "Pending<{}>({depth})", std::any::type_name::<T>()),
            Err(_) => write!(f, "Pending<{}>(locked)", std::any::type_name::<T>()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Appends, like an event.
    #[derive(Debug, PartialEq)]
    struct Happening(u32);
    impl Supersedes for Happening {}

    /// Replaces by name, like a recompiled module.
    #[derive(Debug, PartialEq)]
    struct Recompiled {
        module: &'static str,
        version: u32,
    }
    impl Supersedes for Recompiled {
        fn supersedes(&self, earlier: &Self) -> bool {
            self.module == earlier.module
        }
    }

    #[test]
    fn a_queued_item_comes_back_once() {
        let pending = Pending::new();
        pending.push(Happening(1));

        assert_eq!(pending.len(), 1);
        assert_eq!(pending.drain(), vec![Happening(1)]);
        assert!(pending.is_empty(), "draining takes it");
    }

    /// The default is to append: two collisions in one frame are two events,
    /// and a producer that meant to send one should send one.
    #[test]
    fn items_that_supersede_nothing_all_arrive() {
        let pending = Pending::new();
        pending.push(Happening(1));
        pending.push(Happening(1));

        assert_eq!(pending.drain().len(), 2);
    }

    /// **Two saves between frames are one reload.** Applying the older after
    /// the newer would leave the game running code the author already replaced.
    #[test]
    fn an_item_that_supersedes_replaces_in_place() {
        let pending = Pending::new();
        pending.push(Recompiled {
            module: "ai/guard.erg",
            version: 1,
        });
        pending.push(Recompiled {
            module: "ai/guard.erg",
            version: 2,
        });

        let drained = pending.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].version, 2, "the later save is the one kept");
    }

    #[test]
    fn superseding_is_per_item_not_per_queue() {
        let pending = Pending::new();
        pending.push(Recompiled {
            module: "ai/guard.erg",
            version: 1,
        });
        pending.push(Recompiled {
            module: "loot/chest.erg",
            version: 1,
        });

        assert_eq!(pending.drain().len(), 2);
    }

    /// Two handles are one queue — that is the whole point of the `Arc`.
    #[test]
    fn a_clone_shares_the_queue() {
        let producer = Pending::new();
        let consumer = producer.clone();
        producer.push(Happening(7));

        assert_eq!(consumer.drain(), vec![Happening(7)]);
        assert!(producer.is_empty());
    }
}
