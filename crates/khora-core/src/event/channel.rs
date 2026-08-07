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

//! Something happened; someone reads it later.
//!
//! The engine had written this six times before this file existed: the script
//! reload queue, the asset watcher's backlog, the script event queue, the
//! command buffer, the telemetry channel, the budget channel. Only two things
//! actually separated them — **how a read consumes** (a cursor, or a
//! destructive take) and **what happens when it is full** — so those are the
//! only two things this is parameterised on.
//!
//! # One channel per event type, never one bus
//!
//! Each instance lives in [`Resources`](crate::runtime::Resources) under its own
//! `TypeId`, which is what an agent names in
//! [`Agent::contention`](crate::agent::Agent::contention). A single global bus
//! would make every agent that reads any event declare the same resource,
//! so every one of them would conflict with every other and the whole set would
//! serialise — trading N queues for one global lock, and losing exactly the
//! parallelism the contention contract buys.
//!
//! # The cursor belongs to the reader
//!
//! Subscribing costs the reader a [`Cursor`] field and costs the channel
//! nothing. That is not a detail: if the channel held the cursors, advancing one
//! would be a write, two readers would contend on the lock, and they would
//! serialise despite reading disjoint answers to the same question. With the
//! cursor outside, [`read`](Channel::read) takes the lock **shared** and several
//! agents genuinely read at once — which is what makes declaring it as a
//! `reading` rather than a `locking` honest.
//!
//! # Bounded, and the losses are counted
//!
//! Every unbounded queue in the engine today is one somebody meant to bound
//! later. Capacity is therefore a constructor argument with no default, and
//! whatever is dropped is counted where [`dropped`](Channel::dropped) can be
//! read — a queue that discards in silence is a queue whose failure nobody can
//! see.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

/// Whether a newly sent item makes an already-queued one obsolete.
///
/// The rule lives on the item because it is a fact about the item: two saves of
/// the same file between frames are one reload, and two collisions in one frame
/// are two collisions. Nothing about a queue decides that.
pub trait Supersedes {
    /// Whether this type ever supersedes anything.
    ///
    /// `false` — the default — lets [`send`](Channel::send) skip the scan
    /// entirely. It is a compile-time answer on purpose: a type that never
    /// coalesces would otherwise pay a comparison against every queued item on
    /// every send, which for a few hundred contacts a frame is quadratic work
    /// to discover that the answer is always no.
    const COALESCES: bool = false;

    /// Whether `self`, arriving now, replaces `earlier` in place.
    fn supersedes(&self, earlier: &Self) -> bool {
        let _ = earlier;
        false
    }
}

/// What a full channel does with the next item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenFull {
    /// The oldest item leaves to make room.
    ///
    /// For a stream where the recent matters more than the complete — input,
    /// filesystem changes. A reader that has fallen behind loses history, not
    /// its place.
    DropOldest,
    /// The new item is refused.
    ///
    /// For a stream where an old item already queued is worth more than a new
    /// one — anything a consumer must answer in order.
    Reject,
}

/// Where one reader has read to.
///
/// A position in the stream as a whole, not an index into storage, so it stays
/// meaningful after the channel has dropped what it no longer holds. Default is
/// the beginning, which is what makes a reader that subscribes late still hear
/// whatever is retained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor(u64);

/// A typed stream: someone sends, one or several read later.
pub struct Channel<T> {
    ring: Arc<RwLock<Ring<T>>>,
    /// A cursor for each reader that has nowhere to keep one.
    ///
    /// Behind its own lock, not the ring's, so naming a cursor never delays a
    /// read of the stream: several readers still take `ring` shared.
    named: Arc<Mutex<HashMap<&'static str, Cursor>>>,
}

struct Ring<T> {
    items: VecDeque<T>,
    /// Stream position of `items[0]`.
    base: u64,
    capacity: usize,
    when_full: WhenFull,
    dropped: u64,
}

// Written out rather than derived: a derive would demand `T: Clone`, which an
// `Arc` handle never needs. Cloning shares the stream, it does not copy it.
impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            ring: Arc::clone(&self.ring),
            named: Arc::clone(&self.named),
        }
    }
}

impl<T: Supersedes> Channel<T> {
    /// A channel holding at most `capacity` unread items.
    ///
    /// There is no unbounded constructor, and that is the point: every queue in
    /// this engine that grew without a limit was one where nobody chose.
    pub fn bounded(capacity: usize, when_full: WhenFull) -> Self {
        assert!(capacity > 0, "a channel that can hold nothing is a bug");
        Self {
            ring: Arc::new(RwLock::new(Ring {
                items: VecDeque::with_capacity(capacity),
                base: 0,
                capacity,
                when_full,
                dropped: 0,
            })),
            named: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Sends an item, replacing whatever it supersedes.
    pub fn send(&self, item: T) {
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; dropping an item",
                std::any::type_name::<T>()
            );
            return;
        };

        if T::COALESCES {
            if let Some(earlier) = ring.items.iter_mut().find(|held| item.supersedes(held)) {
                // Replaced in place rather than moved to the back: a reader
                // that has already passed this position has seen the older
                // one, and moving it would show it the same subject twice.
                *earlier = item;
                return;
            }
        }

        if ring.items.len() == ring.capacity {
            match ring.when_full {
                WhenFull::DropOldest => {
                    ring.items.pop_front();
                    ring.base += 1;
                    ring.dropped += 1;
                }
                WhenFull::Reject => {
                    ring.dropped += 1;
                    return;
                }
            }
        }
        ring.items.push_back(item);
    }

    /// Everything `cursor` has not seen, advancing it past what is returned.
    ///
    /// Takes the lock **shared**, so several readers run at once. A cursor left
    /// behind by a drop resumes at the oldest item still held rather than
    /// skipping into nothing — it loses history, never its place.
    pub fn read(&self, cursor: &mut Cursor) -> Vec<T>
    where
        T: Clone,
    {
        let Ok(ring) = self.ring.read() else {
            log::error!(
                "the {} channel is poisoned; a reader hears nothing",
                std::any::type_name::<T>()
            );
            return Vec::new();
        };

        let from = cursor.0.max(ring.base);
        let end = ring.base + ring.items.len() as u64;
        cursor.0 = end;

        ring.items
            .iter()
            .skip((from - ring.base) as usize)
            .cloned()
            .collect()
    }

    /// Everything `reader` has not seen, remembering its position by name.
    ///
    /// For a reader with nowhere to keep a [`Cursor`] — a `DataSystem` is a
    /// free function, and so is a hot-reload pump. Prefer [`read`](Self::read)
    /// wherever the reader is a struct: this one takes a second lock to find
    /// the cursor, and two callers under the same name would share a position
    /// and starve each other.
    pub fn read_for(&self, reader: &'static str) -> Vec<T>
    where
        T: Clone,
    {
        let Ok(mut named) = self.named.lock() else {
            log::error!(
                "the {} channel's cursors are poisoned; {reader} hears nothing",
                std::any::type_name::<T>()
            );
            return Vec::new();
        };
        let mut cursor = *named.entry(reader).or_default();
        let unread = self.read(&mut cursor);
        named.insert(reader, cursor);
        unread
    }

    /// Takes everything, leaving the channel empty.
    ///
    /// For the single-consumer case, where moving beats cloning and `T` need
    /// not be [`Clone`]. Any cursor is left pointing past what was taken, so a
    /// reader that also subscribes does not see it again.
    pub fn drain(&self) -> Vec<T> {
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; the drain finds nothing",
                std::any::type_name::<T>()
            );
            return Vec::new();
        };
        ring.base += ring.items.len() as u64;
        ring.items.drain(..).collect()
    }

    /// How many sends were lost, since the channel was made.
    ///
    /// Not a diagnostic afterthought: a channel that silently discards is a
    /// failure nobody can observe, and this is what a monitor reads to notice.
    pub fn dropped(&self) -> u64 {
        self.ring.read().map(|ring| ring.dropped).unwrap_or(0)
    }

    /// How many unread items are held.
    pub fn len(&self) -> usize {
        self.ring.read().map(|ring| ring.items.len()).unwrap_or(0)
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> std::fmt::Debug for Channel<T> {
    /// Reports the depth without blocking on the lock — a `Debug` that waits
    /// for a writer turns a log line into a stall.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ring.try_read() {
            Ok(ring) => write!(
                f,
                "Channel<{}>({}, dropped {})",
                std::any::type_name::<T>(),
                ring.items.len(),
                ring.dropped
            ),
            Err(_) => write!(f, "Channel<{}>(locked)", std::any::type_name::<T>()),
        }
    }
}
