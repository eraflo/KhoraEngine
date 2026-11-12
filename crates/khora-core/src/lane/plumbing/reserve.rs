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

//! A family's reserve: what its runs staged, kept across frames until a
//! frame's `Boundary` takes it.
//!
//! A lane writes a `#[staged]` output into the [`Staging`] of its run. When
//! the run is over, the scheduler hands that staging to the runtime's
//! [`Reserve`] at the next frame start — never in the middle of a frame — and
//! a `TickPhase::Boundary` DataSystem takes what the reserve holds for its
//! type. An output that a newer one of the same key
//! ([`Supersedes`]) replaces while it waits is never applied: it goes through
//! its [`Staged::abandon`].

use std::any::TypeId;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::event::Supersedes;

use crate::lane::staging::{Queues, Staging};

/// An output a lane stages: kept in the reserve until a frame's `Boundary`
/// takes it.
///
/// Its [`Supersedes`] says which waiting output a newer one replaces — the
/// same file compiled twice, the same asset decoded twice. The replaced one is
/// never applied: it goes through [`abandon`](Self::abandon), which gives
/// back what it holds (a reservation, a handle). An output that holds nothing
/// to give back abandons by being dropped.
pub trait Staged: Supersedes + Sized + Send + 'static {
    /// Gives back what `self` holds, once a newer output replaced it before
    /// it was applied.
    fn abandon(self) {}
}

/// What the background runs staged and no `Boundary` took yet, by type.
///
/// Held by the runtime as a resource; a clone shares the same reserve. Its
/// lock is a leaf: held only to add or take, never across a call out — an
/// abandon runs while it is held, and gives back what it holds without
/// reaching the reserve.
#[derive(Default, Clone)]
pub struct Reserve {
    queues: Arc<Mutex<Queues>>,
    /// What the host staged since the last frame start, in order: it lands
    /// at the next one, after the background runs that ended.
    from_host: Arc<Mutex<Vec<Staging>>>,
}

impl std::fmt::Debug for Reserve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reserve")
            .field("types", &self.lock().len())
            .finish()
    }
}

impl Reserve {
    /// An empty reserve.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes in what one run staged: each output waits behind those of its
    /// type already held, and replaces — abandoning it — a waiting one it
    /// supersedes. Outputs staged under a [`Lease`](crate::lane::Lease) that no
    /// longer holds are abandoned instead. What the scheduler does with a
    /// run's staging at the frame start after the run ended.
    pub fn receive(&self, staging: Staging) {
        for batch in staging.into_batches() {
            // Asked with the reserve's lock not held: a lease may lock.
            if batch.lease().is_some_and(|lease| !lease.holds()) {
                batch.abandon();
                continue;
            }
            batch.join(&mut self.lock());
        }
    }

    /// Stages `staging` from the host — a mount, an unmount — to land at the
    /// next frame start, after what the background runs that ended by then
    /// staged: the host's action, the later one, has the last word over a run
    /// it overtook.
    pub fn stage_from_host(&self, staging: Staging) {
        self.from_host
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(staging);
    }

    /// Lands what the host staged since the last frame start, in the order
    /// it was staged. What the scheduler's frame start does once the ended
    /// background runs are received.
    pub fn land_from_host(&self) {
        let staged = std::mem::take(
            &mut *self
                .from_host
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for staging in staged {
            self.receive(staging);
        }
    }

    /// Takes everything the reserve holds of `T`, oldest first, leaving none.
    pub fn take<T: Staged>(&self) -> Vec<T> {
        self.lock()
            .iter_mut()
            .find(|(ty, _)| *ty == TypeId::of::<T>())
            .and_then(|(_, queue)| queue.downcast_mut::<Vec<T>>())
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// How many outputs of `T` wait in the reserve.
    pub fn len<T: Staged>(&self) -> usize {
        self.lock()
            .iter()
            .find(|(ty, _)| *ty == TypeId::of::<T>())
            .and_then(|(_, queue)| queue.downcast_ref::<Vec<T>>())
            .map_or(0, Vec::len)
    }

    /// Whether no output of `T` waits in the reserve.
    pub fn is_empty<T: Staged>(&self) -> bool {
        self.len::<T>() == 0
    }

    // The queues are whole after every write, so a panic elsewhere while the
    // lock was held leaves nothing half-written: read through the poison.
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
