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

//! Engine collider handles that outlive the Rapier collider they first named.
//!
//! Rapier 0.32 keeps a contact pair's old points across `set_shape`, so a body
//! grown in place stays sunk in the ground. A collider holding contacts is
//! therefore replaced when its shape changes; its engine handle names the new
//! one through the table here, and the `Stopped`/`Started` the swap raises with
//! what it still touches is dropped, so no script hears a false leave.

use std::collections::HashMap;

use khora_core::physics::{ColliderHandle, CollisionEvent};

use super::{from_rapier_cl_handle, to_rapier_cl_handle};

/// Rapier's own collider handle, as opposed to the engine's [`ColliderHandle`].
pub(super) type RapierCollider = rapier3d::geometry::ColliderHandle;

/// A collider replaced since the last step, and who it was touching then.
struct Replacement {
    old: RapierCollider,
    new: RapierCollider,
    touching: Vec<RapierCollider>,
}

/// Which Rapier collider an engine handle names, once one has been replaced.
#[derive(Default)]
pub(super) struct ColliderAliases {
    /// The Rapier collider behind an engine handle whose collider was
    /// replaced. Every other handle is its Rapier handle, packed.
    aliases: HashMap<ColliderHandle, RapierCollider>,
    /// The other way round.
    aliased: HashMap<RapierCollider, ColliderHandle>,
    /// Replacements whose contacts the next step re-finds.
    pending: Vec<Replacement>,
}

impl ColliderAliases {
    /// Returns the Rapier collider an engine handle names.
    pub(super) fn rapier_of(&self, handle: ColliderHandle) -> RapierCollider {
        self.aliases
            .get(&handle)
            .copied()
            .unwrap_or_else(|| to_rapier_cl_handle(handle))
    }

    /// Returns the engine handle of a Rapier collider.
    pub(super) fn khora_of(&self, handle: RapierCollider) -> ColliderHandle {
        self.aliased
            .get(&handle)
            .copied()
            .unwrap_or_else(|| from_rapier_cl_handle(handle))
    }

    /// Records that `handle` names `new`, which replaced `old` while it touched
    /// `touching`.
    pub(super) fn replaced(
        &mut self,
        handle: ColliderHandle,
        old: RapierCollider,
        new: RapierCollider,
        touching: Vec<RapierCollider>,
    ) {
        self.aliased.remove(&old);
        self.aliases.insert(handle, new);
        self.aliased.insert(new, handle);
        // Replaced twice before a step: the first stand-in never met anyone.
        match self.pending.iter_mut().find(|r| r.new == old) {
            Some(replacement) => replacement.new = new,
            None => self.pending.push(Replacement { old, new, touching }),
        }
    }

    /// Forgets `handle`, naming `collider`, once it is removed.
    pub(super) fn removed(&mut self, handle: ColliderHandle, collider: RapierCollider) {
        self.aliases.remove(&handle);
        self.aliased.remove(&collider);
        self.pending.retain(|r| r.new != collider);
    }

    /// Rewrites a step's events in engine handles, without the leave-and-return a
    /// replacement raised with what it still touches.
    pub(super) fn settle(&mut self, events: &mut Vec<CollisionEvent>) {
        if self.pending.is_empty() && self.aliased.is_empty() {
            return;
        }
        let mut raw: Vec<(bool, RapierCollider, RapierCollider)> = events
            .drain(..)
            .map(|event| match event {
                CollisionEvent::Started(a, b) => {
                    (true, to_rapier_cl_handle(a), to_rapier_cl_handle(b))
                }
                CollisionEvent::Stopped(a, b) => {
                    (false, to_rapier_cl_handle(a), to_rapier_cl_handle(b))
                }
            })
            .collect();
        let between = |e: &(bool, RapierCollider, RapierCollider), x, y| {
            (e.1 == x && e.2 == y) || (e.1 == y && e.2 == x)
        };
        for r in self.pending.drain(..) {
            let resumed: Vec<RapierCollider> = r
                .touching
                .iter()
                .copied()
                .filter(|p| raw.iter().any(|e| e.0 && between(e, r.new, *p)))
                .collect();
            raw.retain(|e| {
                !resumed
                    .iter()
                    .any(|p| (e.0 && between(e, r.new, *p)) || (!e.0 && between(e, r.old, *p)))
            });
            // What is left about the old collider is about the one standing in.
            for e in &mut raw {
                if e.1 == r.old {
                    e.1 = r.new;
                }
                if e.2 == r.old {
                    e.2 = r.new;
                }
            }
        }
        events.extend(raw.into_iter().map(|(started, a, b)| {
            let (a, b) = (self.khora_of(a), self.khora_of(b));
            if started {
                CollisionEvent::Started(a, b)
            } else {
                CollisionEvent::Stopped(a, b)
            }
        }));
    }
}
