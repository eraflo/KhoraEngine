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

//! What one run staged, on its way to the family's
//! [`Reserve`](crate::lane::reserve::Reserve).

use std::any::{Any, TypeId};

use crate::lane::lease::Lease;
use crate::lane::reserve::Staged;

/// The reserve's queues: one per output type, each a `Vec` of that type.
pub(super) type Queues = Vec<(TypeId, Box<dyn Any + Send>)>;

/// One type's outputs of a run, on their way to the reserve. A trait object
/// so a staging holds every type a run staged, and each knows how to join its
/// own queue in the reserve.
pub(super) trait Batch: Send {
    /// Whether the batch holds nothing.
    fn is_empty(&self) -> bool;
    /// Joins `queues` — the reserve's, by type — replacing and abandoning a
    /// waiting output each new one supersedes.
    fn join(self: Box<Self>, queues: &mut Queues);
    /// Adds the items of `other`, a batch of the same type, after its own.
    fn extend(&mut self, other: Box<dyn Any + Send>);
    /// The type it carries.
    fn type_of(&self) -> TypeId;
    /// What the batch holds to, if anything.
    fn lease(&self) -> Option<&Lease>;
    /// Gives back what every item holds: the batch is not taken in.
    fn abandon(self: Box<Self>);
}

/// The outputs of one type, under a lease or not.
pub(super) struct Typed<T: Staged>(pub(super) Vec<T>, pub(super) Option<Lease>);

impl<T: Staged> Batch for Typed<T> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn lease(&self) -> Option<&Lease> {
        self.1.as_ref()
    }

    fn abandon(self: Box<Self>) {
        for item in self.0 {
            item.abandon();
        }
    }

    fn join(self: Box<Self>, queues: &mut Queues) {
        let at = match queues.iter().position(|(ty, _)| *ty == TypeId::of::<T>()) {
            Some(at) => at,
            None => {
                queues.push((TypeId::of::<T>(), Box::new(Vec::<T>::new())));
                queues.len() - 1
            }
        };
        let Some(queue) = queues[at].1.downcast_mut::<Vec<T>>() else {
            // The queue at a type's id holds that type, by construction.
            return;
        };
        for item in self.0 {
            supersede(queue, item);
        }
    }

    fn extend(&mut self, other: Box<dyn Any + Send>) {
        if let Ok(other) = other.downcast::<Typed<T>>() {
            self.0.extend(other.0);
        }
    }

    fn type_of(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

/// Adds `item` to `queue`: in place of the waiting output it supersedes,
/// which is abandoned, or after the others.
pub(super) fn supersede<T: Staged>(queue: &mut Vec<T>, item: T) {
    if T::COALESCES {
        if let Some(at) = queue.iter().position(|held| item.supersedes(held)) {
            let replaced = std::mem::replace(&mut queue[at], item);
            replaced.abandon();
            return;
        }
    }
    queue.push(item);
}

/// What one run of a lane staged through its `#[staged]` outputs, in the
/// order it staged them. The scheduler hands it to the
/// [`Reserve`](crate::lane::reserve::Reserve) at the frame start after the run
/// ended.
#[derive(Default)]
pub struct Staging {
    batches: Vec<Box<dyn Batch>>,
}

impl std::fmt::Debug for Staging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Staging")
            .field("types", &self.batches.len())
            .finish()
    }
}

impl Staging {
    /// Nothing staged.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `items`, after what was staged before.
    pub fn stage<T: Staged>(&mut self, items: Vec<T>) {
        if items.is_empty() {
            return;
        }
        // Into the last batch of the type when it holds to no lease; a new
        // one otherwise, so a lease never covers what was staged without it.
        match self
            .batches
            .iter_mut()
            .rev()
            .find(|batch| batch.type_of() == TypeId::of::<T>())
        {
            Some(batch) if batch.lease().is_none() => batch.extend(Box::new(Typed(items, None))),
            _ => self.batches.push(Box::new(Typed(items, None))),
        }
    }

    /// Adds `items` under `lease`, after what was staged before: the reserve
    /// takes them in only if the lease still holds when it receives them, and
    /// abandons them otherwise.
    pub fn stage_while<T: Staged>(&mut self, items: Vec<T>, lease: Lease) {
        if !items.is_empty() {
            self.batches.push(Box::new(Typed(items, Some(lease))));
        }
    }

    /// Whether nothing was staged.
    pub fn is_empty(&self) -> bool {
        self.batches.iter().all(|batch| batch.is_empty())
    }

    /// The batches, by type, for the reserve to take in.
    pub(super) fn into_batches(self) -> Vec<Box<dyn Batch>> {
        self.batches
    }
}
