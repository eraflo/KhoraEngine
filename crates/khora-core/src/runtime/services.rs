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

//! `Services` — typed container for engine **services** (long-lived
//! stateful objects with rich business APIs).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy.

use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Container of engine services — concrete stateful objects with rich APIs
/// (asset loading, serialization, telemetry, DCC orchestration).
///
/// **Admission criteria.** A service is registered here when it is a
/// concrete type with a non-trivial business API (≥ 3 methods that make
/// sense together), lives for the engine lifetime, and is invoked by name.
/// Trait implementations belong in [`crate::runtime::Backends`]. Plain
/// shared state belongs in [`crate::runtime::Resources`].
///
/// API mirrors the legacy `ServiceRegistry` (drop-in replacement).
#[derive(Default)]
pub struct Services {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    parent: Option<Arc<Services>>,
}

impl Services {
    /// Creates an empty container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            parent: None,
        }
    }

    /// Creates a container that delegates lookups to `parent` when a key
    /// is absent locally. Local inserts shadow parent entries.
    #[must_use]
    pub fn with_parent(parent: Arc<Services>) -> Self {
        Self {
            inner: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Inserts a service, keyed by `T`'s `TypeId`. Replaces any prior
    /// entry of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, service: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(service));
    }

    /// Returns a borrow of the registered service, walking the parent
    /// chain on miss.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
            .or_else(|| self.parent.as_deref()?.get::<T>())
    }

    /// Returns a borrow of the registered service, panicking if absent.
    pub fn require<T: Send + Sync + 'static>(&self) -> &T {
        self.get::<T>().unwrap_or_else(|| {
            panic!(
                "Services: required service `{}` is not registered",
                type_name::<T>()
            )
        })
    }

    /// Reports whether a service of the given type is registered.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.inner.contains_key(&TypeId::of::<T>())
            || self
                .parent
                .as_deref()
                .map(|p| p.contains::<T>())
                .unwrap_or(false)
    }

    /// Number of services registered locally (excluding parent chain).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether no services are registered locally.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services")
            .field("registered", &self.inner.len())
            .field("has_parent", &self.parent.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAssetService {
        name: String,
    }
    struct FakeSerializationService;

    #[test]
    fn insert_and_get() {
        let mut s = Services::new();
        s.insert(FakeAssetService {
            name: "assets".into(),
        });
        assert_eq!(s.get::<FakeAssetService>().unwrap().name, "assets");
    }

    #[test]
    fn require_panics_when_absent() {
        let s = Services::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = s.require::<FakeAssetService>().name.len();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parent_chain_delegates() {
        let mut parent = Services::new();
        parent.insert(FakeAssetService {
            name: "from-parent".into(),
        });
        let child = Services::with_parent(Arc::new(parent));
        assert_eq!(child.get::<FakeAssetService>().unwrap().name, "from-parent");
    }

    #[test]
    fn local_shadows_parent() {
        let mut parent = Services::new();
        parent.insert(FakeAssetService {
            name: "parent".into(),
        });
        let mut child = Services::with_parent(Arc::new(parent));
        child.insert(FakeAssetService {
            name: "child".into(),
        });
        assert_eq!(child.get::<FakeAssetService>().unwrap().name, "child");
    }

    #[test]
    fn contains_walks_parent() {
        let mut parent = Services::new();
        parent.insert(FakeSerializationService);
        let child = Services::with_parent(Arc::new(parent));
        assert!(child.contains::<FakeSerializationService>());
    }
}
