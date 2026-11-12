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

//! What a staged output holds to while it waits.
//!
//! A background run works on a world that moves on without it: the project it
//! compiles may be closed, the asset it decodes unloaded, before it ends. Its
//! outputs then answer a question nobody asks any more. A run that knows what
//! its work depends on stages it under a [`Lease`]; the reserve checks the
//! lease when it takes the outputs in — at a frame start, on the thread the
//! host acts on — and abandons them when it no longer holds. Checked there,
//! never in the run, a lease cannot race the host action that breaks it.

use std::sync::Arc;

/// A condition staged outputs hold to: while it holds, they count; once it
/// does not, the reserve abandons them instead of taking them in.
///
/// Checked on the thread that runs the frame start, with the reserve's lock
/// not held: the condition may take a lock of its own, as a leaf.
#[derive(Clone)]
pub struct Lease(Arc<dyn Fn() -> bool + Send + Sync>);

impl Lease {
    /// A lease that holds while `holds` answers yes.
    pub fn new(holds: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(holds))
    }

    /// Whether the lease still holds.
    pub fn holds(&self) -> bool {
        (self.0)()
    }
}

impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease").finish_non_exhaustive()
    }
}

/// Outputs a lane stages in groups, each under the [`Lease`]
/// its work depended on, or none: the field type of a `#[staged]` output
/// whose groups can each stop counting before the run ends — the project
/// one compile was for closed, the asset one decode was for unloaded.
#[derive(Debug)]
pub struct Leases<T> {
    groups: Vec<(Vec<T>, Option<Lease>)>,
}

impl<T> Default for Leases<T> {
    fn default() -> Self {
        Self { groups: Vec::new() }
    }
}

impl<T> Leases<T> {
    /// Adds `items`, under `lease` when there is one, after the groups added
    /// before.
    pub fn push(&mut self, items: Vec<T>, lease: Option<Lease>) {
        self.groups.push((items, lease));
    }

    /// Every item added, group after group, leases put aside.
    pub fn items(&self) -> impl Iterator<Item = &T> {
        self.groups.iter().flat_map(|(items, _)| items)
    }

    /// The groups, in the order they were added.
    pub(crate) fn into_groups(self) -> Vec<(Vec<T>, Option<Lease>)> {
        self.groups
    }

    /// Whether nothing was added.
    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|(items, _)| items.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_lease_answers_what_its_condition_answers_now() {
        let open = Arc::new(AtomicBool::new(true));
        let lease = {
            let open = Arc::clone(&open);
            Lease::new(move || open.load(Ordering::SeqCst))
        };
        assert!(lease.holds());
        open.store(false, Ordering::SeqCst);
        assert!(!lease.holds());
        assert!(!lease.clone().holds(), "a clone asks the same condition");
    }
}
