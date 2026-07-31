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

//! `Resources` — typed container for **engine resources**: long-lived
//! shared state without a service-style API (caches, registries, settings,
//! lookup tables).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy.

use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Container of engine resources — long-lived shared state that is
/// principally data (with at most trivial accessors), not a service.
///
/// **Admission criteria.** A resource lives here when it is principally
/// data with trivial accessors (HashMaps, settings, caches), is shared
/// between several lanes / agents / data systems but has no rich business
/// API, and is *long-lived* (per-tick state belongs in
/// [`OutputDeck`](crate::lane::OutputDeck) or `LaneContext`).
///
/// Many resources will internally use `Arc<RwLock<…>>` or `Arc<Mutex<…>>`
/// to support concurrent access. That is the resource's responsibility,
/// not the container's.
///
/// API mirrors the legacy `ServiceRegistry` (drop-in replacement).
#[derive(Default)]
pub struct Resources {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    parent: Option<Arc<Resources>>,
}

impl Resources {
    /// Creates an empty container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            parent: None,
        }
    }

    /// Creates a container that delegates lookups to `parent` when a key
    /// is absent locally.
    #[must_use]
    pub fn with_parent(parent: Arc<Resources>) -> Self {
        Self {
            inner: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Inserts a resource, keyed by `T`'s `TypeId`.
    pub fn insert<T: Send + Sync + 'static>(&mut self, resource: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(resource));
    }

    /// Returns a borrow of the registered resource, walking the parent chain.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
            .or_else(|| self.parent.as_deref()?.get::<T>())
    }

    /// Returns a borrow of the registered resource, panicking if absent.
    pub fn require<T: Send + Sync + 'static>(&self) -> &T {
        self.get::<T>().unwrap_or_else(|| {
            panic!(
                "Resources: required resource `{}` is not registered",
                type_name::<T>()
            )
        })
    }

    /// Reports whether a resource of the given type is registered.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.inner.contains_key(&TypeId::of::<T>())
            || self
                .parent
                .as_deref()
                .map(|p| p.contains::<T>())
                .unwrap_or(false)
    }

    /// Number of resources registered locally.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether no resources are registered locally.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl std::fmt::Debug for Resources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resources")
            .field("registered", &self.inner.len())
            .field("has_parent", &self.parent.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Debug)]
    struct InputMap {
        bindings: u32,
    }

    #[test]
    fn insert_and_get() {
        let mut r = Resources::new();
        r.insert(InputMap { bindings: 42 });
        assert_eq!(r.get::<InputMap>().unwrap().bindings, 42);
    }

    #[test]
    fn parent_chain_delegates() {
        let mut parent = Resources::new();
        parent.insert(InputMap { bindings: 7 });
        let child = Resources::with_parent(Arc::new(parent));
        assert_eq!(child.get::<InputMap>().unwrap().bindings, 7);
    }
}
