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

//! A family's reserve: what its runs staged, kept until it is applied.
//!
//! Received at a frame start and kept until a `TickPhase::Boundary`
//! `DataSystem` takes it. An output a newer one supersedes while it waits is
//! abandoned, never applied.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use crate::event::Supersedes;
use crate::sync::LockExt;

use crate::lane::staging::{Queues, Staging};

/// An output a lane stages, kept in the reserve until a frame's `Boundary` takes it.
///
/// Its [`Supersedes`] says which waiting output a newer one replaces (the same
/// file compiled twice). The replaced one is never applied: it goes through
/// [`abandon`](Self::abandon).
pub trait Staged: Supersedes + Sized + Send + 'static {
    /// Gives back what `self` holds (a reservation, a handle), as it is never applied.
    ///
    /// Runs while the [`Reserve`]'s lock is held, so it must not reach the
    /// reserve. Defaults to dropping it.
    fn abandon(self) {}
}

/// What the background runs staged and no `Boundary` took yet, by type.
///
/// Held by the runtime as a resource; a clone shares the same reserve. Its
/// lock is a leaf: held only to add or take, never across a call out — an
/// abandon runs under it and gives back what it holds without reaching the
/// reserve.
#[derive(Default, Clone)]
pub struct Reserve {
    // The queues are whole after every write, so a panic elsewhere while the
    // lock was held leaves nothing half-written: read through the poison.
    queues: Arc<Mutex<Queues>>,
    /// What the host staged since the last frame start, in order: it lands
    /// at the next one, after the background runs that ended.
    from_host: Arc<Mutex<Vec<Staging>>>,
}

impl std::fmt::Debug for Reserve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reserve")
            .field("types", &self.queues.lock_recovered().len())
            .finish()
    }
}

impl Reserve {
    /// Returns an empty reserve.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes in what one run staged, behind the outputs of each type already held.
    ///
    /// An output replaces, abandoning it, a waiting one it supersedes. Outputs
    /// under a [`Lease`](crate::lane::Lease) that fails are abandoned instead.
    pub fn receive(&self, staging: Staging) {
        for batch in staging.into_batches() {
            // Asked with the reserve's lock not held: a lease may lock.
            if batch.lease().is_some_and(|lease| !lease.holds()) {
                batch.abandon();
                continue;
            }
            batch.join(&mut self.queues.lock_recovered());
        }
    }

    /// Stages `staging` from the host (a mount, an unmount) to land at the next frame start.
    ///
    /// It lands after the background runs that ended by then, so the host's
    /// action has the last word over a run it overtook.
    pub fn stage_from_host(&self, staging: Staging) {
        self.from_host.lock_recovered().push(staging);
    }

    /// Lands what the host staged since the last frame start, in order.
    ///
    /// Call after the ended background runs were received.
    pub fn land_from_host(&self) {
        let staged = std::mem::take(&mut *self.from_host.lock_recovered());
        for staging in staged {
            self.receive(staging);
        }
    }

    /// Takes everything the reserve holds of `T`, oldest first, leaving none.
    pub fn take<T: Staged>(&self) -> Vec<T> {
        self.queues
            .lock_recovered()
            .iter_mut()
            .find(|(ty, _)| *ty == TypeId::of::<T>())
            .and_then(|(_, queue)| queue.downcast_mut::<Vec<T>>())
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Returns how many outputs of `T` wait in the reserve.
    pub fn len<T: Staged>(&self) -> usize {
        self.queues
            .lock_recovered()
            .iter()
            .find(|(ty, _)| *ty == TypeId::of::<T>())
            .and_then(|(_, queue)| queue.downcast_ref::<Vec<T>>())
            .map_or(0, Vec::len)
    }

    /// Returns whether no output of `T` waits in the reserve.
    pub fn is_empty<T: Staged>(&self) -> bool {
        self.len::<T>() == 0
    }
}
