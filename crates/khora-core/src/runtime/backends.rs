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

//! `Backends` — typed container for **engine backends** (concrete
//! implementations of abstract traits like [`RenderSystem`],
//! [`PhysicsProvider`], [`AudioDevice`], [`LayoutSystem`]).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy.
//!
//! Backends are swappable by design — that is precisely why they live
//! behind a trait. Game devs and plugins may register their own backends
//! (e.g. a custom `NetworkProvider`) using the same API.
//!
//! # Convention
//!
//! Always register under the trait-object type, not the concrete impl:
//!
//! ```ignore
//! backends.insert::<Arc<Mutex<Box<dyn PhysicsProvider>>>>(physics_arc);
//! let physics = backends.get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>();
//! ```
//!
//! [`RenderSystem`]: crate::renderer::RenderSystem
//! [`PhysicsProvider`]: crate::physics::PhysicsProvider
//! [`AudioDevice`]: crate::audio::device::AudioDevice
//! [`LayoutSystem`]: crate::ui::LayoutSystem

use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

/// Container of engine backends — concrete impls of abstract traits.
///
/// API mirrors the legacy `ServiceRegistry` (drop-in replacement).
#[derive(Default)]
pub struct Backends {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    parent: Option<Arc<Backends>>,
}

impl Backends {
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
    pub fn with_parent(parent: Arc<Backends>) -> Self {
        Self {
            inner: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Inserts a backend, keyed by `T`'s `TypeId`. Convention: `T` is
    /// the trait-object type (e.g. `Arc<Mutex<Box<dyn PhysicsProvider>>>`).
    pub fn insert<T: Send + Sync + 'static>(&mut self, backend: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(backend));
    }

    /// Returns a borrow of the registered backend, walking the parent chain.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
            .or_else(|| self.parent.as_deref()?.get::<T>())
    }

    /// Returns a borrow of the registered backend, panicking if absent.
    /// Use for backends mandatory at engine startup.
    pub fn require<T: Send + Sync + 'static>(&self) -> &T {
        self.get::<T>().unwrap_or_else(|| {
            panic!(
                "Backends: required backend `{}` is not registered",
                type_name::<T>()
            )
        })
    }

    /// Reports whether a backend of the given type is registered.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.inner.contains_key(&TypeId::of::<T>())
            || self
                .parent
                .as_deref()
                .map(|p| p.contains::<T>())
                .unwrap_or(false)
    }

    /// Number of backends registered locally.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether no backends are registered locally.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl std::fmt::Debug for Backends {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backends")
            .field("registered", &self.inner.len())
            .field("has_parent", &self.parent.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    trait FakePhysics: Send + Sync {
        fn name(&self) -> &str;
    }
    struct Rapier;
    impl FakePhysics for Rapier {
        fn name(&self) -> &str {
            "rapier"
        }
    }

    #[test]
    fn register_and_lookup_trait_object() {
        let mut b = Backends::new();
        let provider: Arc<Mutex<Box<dyn FakePhysics>>> = Arc::new(Mutex::new(Box::new(Rapier)));
        b.insert(provider);

        let got = b.require::<Arc<Mutex<Box<dyn FakePhysics>>>>();
        assert_eq!(got.lock().unwrap().name(), "rapier");
    }

    #[test]
    fn require_panics_when_absent() {
        let b = Backends::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = b.require::<Arc<Mutex<Box<dyn FakePhysics>>>>();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parent_chain_delegates() {
        let mut parent = Backends::new();
        let provider: Arc<Mutex<Box<dyn FakePhysics>>> = Arc::new(Mutex::new(Box::new(Rapier)));
        parent.insert(provider);
        let child = Backends::with_parent(Arc::new(parent));
        assert!(child.contains::<Arc<Mutex<Box<dyn FakePhysics>>>>());
    }
}
