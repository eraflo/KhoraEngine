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

//! Something happened; someone reads it later: a bounded, typed [`Channel`].
//!
//! # One channel per event type, never one bus
//!
//! Each channel is its own [`Resources`](crate::runtime::Resources) entry, so
//! an agent declares only the events it reads; one global bus would make
//! every reader contend with every other.
//!
//! # The cursor belongs to the reader
//!
//! With the [`Cursor`] outside the channel, [`read`](Channel::read) takes the
//! lock shared and several agents read at once, so a reader declares a
//! `reading`, not a `locking`.
//!
//! # Bounded, and the losses are counted
//!
//! Capacity has no default. What a full channel drops is counted by
//! [`dropped`](Channel::dropped), what a reader discards by `discarded`, so a
//! monitor can see both.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

/// A type-erased channel that takes back, at its front, what a drain took.
///
/// A fetch refused after its `#[take]` gives the requests back through it.
pub trait Restore: Sync {
    /// Puts back `items`, a `Vec` of the channel's items drained from it; an
    /// item of another type is dropped.
    fn restore_any(&self, items: Box<dyn std::any::Any + Send>);
}

impl<T: Supersedes + Send + Sync + 'static> Restore for Channel<T> {
    fn restore_any(&self, items: Box<dyn std::any::Any + Send>) {
        if let Ok(items) = items.downcast::<Vec<T>>() {
            self.restore(*items);
        }
    }
}

/// Whether a newly sent item makes an already-queued one obsolete.
///
/// A fact about the item, not the queue: two saves of one file between frames
/// are one reload, two collisions in one frame are two collisions.
pub trait Supersedes {
    /// Whether this type ever supersedes anything.
    ///
    /// `false`, the default, lets [`send`](Channel::send) skip the scan
    /// against every queued item, quadratic work for a busy stream.
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
/// A position in the whole stream, not an index into storage, so it survives
/// what the channel drops. The default is the beginning: a reader that
/// subscribes late still hears whatever is retained.
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
    /// Unread items discarded on purpose, apart from overflow.
    discarded: u64,
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
    /// Returns a channel holding at most `capacity` unread items.
    ///
    /// There is no unbounded constructor: every queue chooses its limit.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is 0.
    pub fn bounded(capacity: usize, when_full: WhenFull) -> Self {
        assert!(capacity > 0, "a channel that can hold nothing is a bug");
        Self {
            ring: Arc::new(RwLock::new(Ring {
                items: VecDeque::with_capacity(capacity),
                base: 0,
                capacity,
                when_full,
                dropped: 0,
                discarded: 0,
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
    /// For a reader with nowhere to keep a [`Cursor`], such as a `DataSystem`.
    /// Prefer [`read`](Self::read) otherwise: this takes a second lock, and two
    /// callers under one name share a position and starve each other.
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

    /// Puts `items`, taken by a drain, back at the front of the channel, in order.
    ///
    /// They are held whatever the capacity. An item superseded by one sent
    /// since is dropped, as it would have been in [`send`](Self::send).
    pub fn restore(&self, mut items: Vec<T>) {
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; {} item(s) given back are lost",
                std::any::type_name::<T>(),
                items.len()
            );
            return;
        };
        if T::COALESCES {
            items.retain(|item| !ring.items.iter().any(|held| held.supersedes(item)));
        }
        ring.base = ring.base.saturating_sub(items.len() as u64);
        for item in items.into_iter().rev() {
            ring.items.push_front(item);
        }
    }

    /// Keeps only the held items `keep` accepts.
    ///
    /// For a queue its consumer drains — a reload, a declaration — that the
    /// sender takes something back from before it is read. A cursor a reader
    /// holds past a removed item moves by it: such a reader may miss what
    /// followed, so a channel read by cursor is not withdrawn from.
    pub fn retain(&self, mut keep: impl FnMut(&T) -> bool) {
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; nothing is withdrawn",
                std::any::type_name::<T>()
            );
            return;
        };
        ring.items.retain(|item| keep(item));
    }

    /// Discards every unread item, as a drain would, leaving every cursor past them.
    ///
    /// Counted by [`discarded`](Self::discarded), never by
    /// [`dropped`](Self::dropped): nothing overflowed.
    pub fn discard_unread(&self) {
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; nothing is discarded",
                std::any::type_name::<T>()
            );
            return;
        };
        let unread = ring.items.len() as u64;
        ring.base += unread;
        ring.discarded += unread;
        ring.items.clear();
    }

    /// Moves the named `reader` past everything it has not read, as if it had read it.
    ///
    /// The items stay for the other readers; what this reader skipped is
    /// counted by [`discarded`](Self::discarded).
    pub fn skip_to_end(&self, reader: &'static str) {
        let Ok(mut named) = self.named.lock() else {
            log::error!(
                "the {} channel's cursors are poisoned; {reader} keeps its place",
                std::any::type_name::<T>()
            );
            return;
        };
        let Ok(mut ring) = self.ring.write() else {
            log::error!(
                "the {} channel is poisoned; {reader} keeps its place",
                std::any::type_name::<T>()
            );
            return;
        };
        let end = ring.base + ring.items.len() as u64;
        let cursor = named.entry(reader).or_default();
        let from = cursor.0.max(ring.base);
        ring.discarded += end - from;
        cursor.0 = end;
    }

    /// How many unread items were discarded on purpose since the channel was made.
    ///
    /// Counts [`discard_unread`](Self::discard_unread) and
    /// [`skip_to_end`](Self::skip_to_end), not overflow.
    pub fn discarded(&self) -> u64 {
        self.ring.read().map(|ring| ring.discarded).unwrap_or(0)
    }

    /// How many sends were lost, since the channel was made.
    ///
    /// What a monitor reads to notice a channel losing items.
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
