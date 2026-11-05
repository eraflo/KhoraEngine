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

//! The items each owner wants projected, and their union.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::Demand;

/// What each owner wants a projection to hold, behind one leaf lock.
///
/// An owner [`announce`](Self::announce)s its items, replacing what it
/// announced before, and [`withdraw`](Self::withdraw)s them. The projection
/// reads [`wanted`](Self::wanted), the sorted union of every owner's items,
/// each once. [`generation`](Self::generation) moves on every change of that
/// union, and never otherwise.
pub struct Interest<D: Demand> {
    /// Every owner's items and their union, behind the one lock.
    state: RwLock<State<D::Item>>,
    _demand: PhantomData<fn() -> D>,
}

/// What the lock guards: the entries, and the union they make.
struct State<T> {
    /// What each owner announced.
    owners: BTreeMap<String, BTreeSet<T>>,
    /// The union of every owner's items, kept in step with `owners`.
    wanted: BTreeSet<T>,
    /// Moves when `wanted` changes.
    generation: u64,
}

impl<T: Ord + Clone> State<T> {
    /// Rebuilds the union after an entry changed, moving the generation only
    /// when the union is not what it was.
    fn settle(&mut self) {
        let wanted: BTreeSet<T> = self.owners.values().flatten().cloned().collect();
        if wanted != self.wanted {
            self.wanted = wanted;
            self.generation += 1;
        }
    }
}

impl<D: Demand> Default for Interest<D> {
    fn default() -> Self {
        Self {
            state: RwLock::new(State {
                owners: BTreeMap::new(),
                wanted: BTreeSet::new(),
                generation: 0,
            }),
            _demand: PhantomData,
        }
    }
}

impl<D: Demand> std::fmt::Debug for Interest<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interest").finish_non_exhaustive()
    }
}

impl<D: Demand> Interest<D> {
    /// An interest no owner announced anything to.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `owner` wants `items`, replacing what it announced
    /// before.
    pub fn announce(&self, owner: impl Into<String>, items: impl IntoIterator<Item = D::Item>) {
        let items: BTreeSet<D::Item> = items.into_iter().collect();
        let mut state = self.write();
        state.owners.insert(owner.into(), items);
        state.settle();
    }

    /// Forgets what `owner` announced; nothing when it announced nothing.
    pub fn withdraw(&self, owner: &str) {
        let mut state = self.write();
        if state.owners.remove(owner).is_some() {
            state.settle();
        }
    }

    /// Every item some owner wants, sorted, each once.
    pub fn wanted(&self) -> Vec<D::Item> {
        self.read().wanted.iter().cloned().collect()
    }

    /// Whether every one of `items` is wanted; `true` for none.
    pub fn covers(&self, items: &[D::Item]) -> bool {
        let state = self.read();
        items.iter().all(|item| state.wanted.contains(item))
    }

    /// Moves on every change of [`wanted`](Self::wanted), never otherwise.
    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    // Every write settles the union before its guard drops, so a lock
    // poisoned by a panic elsewhere still guards a whole state: read through.
    fn read(&self) -> RwLockReadGuard<'_, State<D::Item>> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, State<D::Item>> {
        self.state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
