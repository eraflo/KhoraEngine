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

//! The allocation channel from the DCC's cold thread to the scheduler's frame.
//!
//! The DCC publishes each epoch's [`Allocation`] as it decides it; the
//! scheduler takes one at its frame start ([`BudgetChannel::sync`]) and lends
//! it to its agents there, so nothing published is seen in the middle of a
//! frame. A live allocation supersedes whatever live one waits: of several
//! published between two frame starts, the last wins. A replayed one is
//! published [in turn](BudgetChannel::publish_in_turn): each is taken at a
//! frame start of its own, in order, however long the frames.

use khora_core::agent::gorna::protocol::Allocation;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};

/// A publication: the allocation, and its place among the ones published.
pub(crate) type Publication = (u64, Arc<Allocation>);

#[derive(Default)]
struct BudgetChannelInner {
    /// What waits for a frame start, oldest first.
    waiting: Mutex<Waiting>,
    /// The allocation in force since the last sync.
    current: RwLock<Option<Publication>>,
}

#[derive(Default)]
struct Waiting {
    /// The publications waiting, each with whether it waits its turn (a
    /// replayed one) or may be superseded (a live one). A live one, if any,
    /// is the last.
    queue: VecDeque<(Publication, bool)>,
    /// How many allocations were published, the last one's place.
    count: u64,
}

impl Waiting {
    /// Queues `allocation`; a live one waiting is superseded first.
    fn push(&mut self, allocation: Allocation, in_turn: bool) {
        if self.queue.back().is_some_and(|(_, in_turn)| !in_turn) {
            self.queue.pop_back();
        }
        self.count += 1;
        self.queue
            .push_back(((self.count, Arc::new(allocation)), in_turn));
    }
}

/// The channel between the DCC, which publishes, and the scheduler, which
/// syncs at each frame start and lends.
///
/// Cloneable: both sides hold the same channel.
#[derive(Clone, Default)]
pub struct BudgetChannel {
    inner: Arc<BudgetChannelInner>,
}

impl BudgetChannel {
    /// A channel no allocation was published on yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cold thread: publishes the allocation the DCC decided. Non-blocking;
    /// the last one published before a [`sync`](Self::sync) wins over the
    /// live ones before it.
    pub fn publish(&self, allocation: Allocation) {
        self.waiting().push(allocation, false);
    }

    /// Cold thread: publishes a replayed allocation, which waits its turn: it
    /// is taken at a frame start of its own, after the ones published in turn
    /// before it, and no later publication supersedes it.
    pub fn publish_in_turn(&self, allocation: Allocation) {
        self.waiting().push(allocation, true);
    }

    /// Whether `other` is this very channel (a clone of it), not one alike.
    pub(crate) fn is(&self, other: &BudgetChannel) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Cold thread: withdraws the replayed allocations still waiting their
    /// turn — a replay stopped lends no more of its epochs. A live one waiting
    /// stays.
    pub fn withdraw_in_turn(&self) {
        self.waiting().queue.retain(|(_, in_turn)| !in_turn);
    }

    /// Hot thread, at a frame start: the oldest publication waiting becomes
    /// the one in force. With nothing new, the one in force stays.
    pub fn sync(&self) {
        let _ = self.take();
    }

    /// [`sync`](Self::sync), handing back what the frame start took, if
    /// anything: the publication — the allocation with its place among the
    /// ones published, so two publications of one allocation are still two —
    /// and whether it was published in turn.
    pub(crate) fn take(&self) -> Option<(Publication, bool)> {
        let next = self.waiting().queue.pop_front()?;
        *self
            .inner
            .current
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(next.0.clone());
        Some(next)
    }

    /// Hot thread: the allocation in force since the last
    /// [`sync`](Self::sync), if any was published.
    pub fn latest(&self) -> Option<Arc<Allocation>> {
        self.inner
            .current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(_, allocation)| Arc::clone(allocation))
    }

    fn waiting(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.inner.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::agent::gorna::protocol::{Epoch, Trigger};

    fn allocation(epoch: u64) -> Allocation {
        Allocation {
            epoch: Epoch(epoch),
            trigger: Trigger::Refresh,
            envelopes: Vec::new(),
            pins: Vec::new(),
            allowed: Vec::new(),
            price: None,
            lever: false,
            late: Vec::new(),
        }
    }

    /// **The hot side sees the latest allocation as of its last sync.** What
    /// the DCC publishes waits for the next sync, so a frame reads one
    /// allocation from its start to its end; of two published between two
    /// syncs, the later wins.
    #[test]
    fn the_hot_side_sees_the_latest_allocation_as_of_its_last_sync() {
        let channel = BudgetChannel::default();
        assert!(channel.latest().is_none(), "nothing published yet");

        channel.publish(allocation(1));
        assert!(channel.latest().is_none(), "published, not yet synced");
        channel.sync();
        assert_eq!(channel.latest().map(|a| a.epoch), Some(Epoch(1)));

        channel.publish(allocation(2));
        channel.publish(allocation(3));
        assert_eq!(
            channel.latest().map(|a| a.epoch),
            Some(Epoch(1)),
            "until the next sync, the frame keeps the one it started with"
        );
        channel.sync();
        assert_eq!(
            channel.latest().map(|a| a.epoch),
            Some(Epoch(3)),
            "the last published wins"
        );

        channel.sync();
        assert_eq!(
            channel.latest().map(|a| a.epoch),
            Some(Epoch(3)),
            "a sync with nothing new keeps the allocation in force"
        );
    }
}
