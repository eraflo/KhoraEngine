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

//! The one type-keyed container behind `Services`, `Backends` and `Resources`.
//!
//! # Why one type and three names
//!
//! The three used to be three files holding the same struct: the same two
//! fields, the same eight methods, the same `Debug`, and — down to the
//! assertion names — the same tests. Only two words differed per copy: what
//! the container is called, and what it calls the things inside it.
//!
//! That taxonomy is real and worth keeping. A service has a rich API, a
//! backend is a swappable implementation, a resource is shared data. But it is
//! a distinction the *reader* needs, not one the machine does, and encoding it
//! by copying a container three times means a fix to lookup or parent-chaining
//! has to be made three times and can be made in two.
//!
//! So the distinction survives as a marker type, and the container is written
//! once. `Services`, `Backends` and `Resources` remain three distinct types —
//! a `Backends` still cannot be passed where a `Services` is expected — and
//! their panic messages and `Debug` output are unchanged, because the marker
//! carries the two words that differed.

use std::any::{type_name, Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

/// What a [`TypedRegistry`] is called, and what it calls its contents.
///
/// Implemented by a zero-sized marker per container. The two constants are the
/// *entire* difference between the three registries — they appear only in
/// `Debug` output and in the panic from [`TypedRegistry::require`].
pub trait RegistryKind: 'static {
    /// The container's name, as it appears in `Debug` and panic messages.
    const NAME: &'static str;
    /// The singular noun for one entry, as it appears in panic messages.
    const NOUN: &'static str;
}

/// A container keyed by the `TypeId` of what it holds, with optional
/// delegation to a parent.
///
/// Not used directly — see the [`Services`](super::Services),
/// [`Backends`](super::Backends) and [`Resources`](super::Resources) aliases,
/// whose documentation states the admission criteria for each.
pub struct TypedRegistry<K: RegistryKind> {
    inner: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    parent: Option<Arc<Self>>,
    /// Carries `K` without occupying a byte; `fn() -> K` keeps the registry
    /// `Send + Sync` regardless of what the marker itself is.
    kind: PhantomData<fn() -> K>,
}

impl<K: RegistryKind> Default for TypedRegistry<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: RegistryKind> TypedRegistry<K> {
    /// Creates an empty container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            parent: None,
            kind: PhantomData,
        }
    }

    /// Creates a container that delegates lookups to `parent` when a key
    /// is absent locally.
    #[must_use]
    pub fn with_parent(parent: Arc<Self>) -> Self {
        Self {
            inner: HashMap::new(),
            parent: Some(parent),
            kind: PhantomData,
        }
    }

    /// Inserts an entry, keyed by `T`'s `TypeId`.
    pub fn insert<T: Send + Sync + 'static>(&mut self, entry: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(entry));
    }

    /// Returns a borrow of the registered entry, walking the parent chain.
    #[must_use]
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
            .or_else(|| self.parent.as_deref()?.get::<T>())
    }

    /// Returns a borrow of the registered entry, panicking if absent.
    pub fn require<T: Send + Sync + 'static>(&self) -> &T {
        self.get::<T>().unwrap_or_else(|| {
            panic!(
                "{}: required {} `{}` is not registered",
                K::NAME,
                K::NOUN,
                type_name::<T>()
            )
        })
    }

    /// Reports whether an entry of the given type is registered.
    #[must_use]
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.inner.contains_key(&TypeId::of::<T>())
            || self
                .parent
                .as_deref()
                .map(|p| p.contains::<T>())
                .unwrap_or(false)
    }

    /// Number of entries registered locally.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether no entries are registered locally.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<K: RegistryKind> std::fmt::Debug for TypedRegistry<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(K::NAME)
            .field("registered", &self.inner.len())
            .field("has_parent", &self.parent.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Backends, Resources, Services};

    #[derive(Debug, PartialEq)]
    struct Thing(u32);

    #[derive(Debug)]
    struct Absent;

    #[test]
    fn insert_and_get() {
        let mut r = Resources::new();
        r.insert(Thing(42));

        assert_eq!(r.get::<Thing>(), Some(&Thing(42)));
    }

    #[test]
    fn a_missing_entry_is_none() {
        assert!(Services::new().get::<Thing>().is_none());
    }

    #[test]
    fn parent_chain_delegates() {
        let mut root = Backends::new();
        root.insert(Thing(7));
        let child = Backends::with_parent(Arc::new(root));

        assert_eq!(child.get::<Thing>(), Some(&Thing(7)));
        assert!(child.contains::<Thing>());
        assert!(child.is_empty(), "delegation is not local ownership");
    }

    #[test]
    fn a_local_entry_shadows_the_parent() {
        let mut root = Resources::new();
        root.insert(Thing(1));
        let mut child = Resources::with_parent(Arc::new(root));
        child.insert(Thing(2));

        assert_eq!(child.get::<Thing>(), Some(&Thing(2)));
    }

    /// `Backends`' documented convention: register under the **trait-object**
    /// type, not the concrete impl, so the entry stays swappable. Keying on
    /// `TypeId` means `Arc<Mutex<Box<dyn T>>>` is its own key — this is what
    /// makes a backend replaceable without every caller learning the new type.
    #[test]
    fn an_entry_can_be_keyed_by_a_trait_object_type() {
        trait FakePhysics: Send + Sync {
            fn name(&self) -> &str;
        }
        struct Rapier;
        impl FakePhysics for Rapier {
            fn name(&self) -> &str {
                "rapier"
            }
        }

        let mut b = Backends::new();
        let provider: Arc<std::sync::Mutex<Box<dyn FakePhysics>>> =
            Arc::new(std::sync::Mutex::new(Box::new(Rapier)));
        b.insert(provider);

        let got = b.require::<Arc<std::sync::Mutex<Box<dyn FakePhysics>>>>();
        assert_eq!(got.lock().expect("uncontended in test").name(), "rapier");
    }

    /// Returns the panic message `require` produced, or fails the test.
    fn message_from_missing_require(call: impl FnOnce()) -> String {
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call))
            .expect_err("require must panic when the entry is absent");
        panic
            .downcast_ref::<String>()
            .expect("panic payload is a formatted String")
            .clone()
    }

    /// Each container names itself and its contents — the only thing the three
    /// aliases do not share, and the only reason the marker carries constants.
    #[test]
    fn each_kind_names_itself_when_it_panics() {
        let services = message_from_missing_require(|| {
            Services::new().require::<Absent>();
        });
        let backends = message_from_missing_require(|| {
            Backends::new().require::<Absent>();
        });
        let resources = message_from_missing_require(|| {
            Resources::new().require::<Absent>();
        });

        assert!(
            services.starts_with("Services: required service"),
            "{services}"
        );
        assert!(
            backends.starts_with("Backends: required backend"),
            "{backends}"
        );
        assert!(
            resources.starts_with("Resources: required resource"),
            "{resources}"
        );
    }

    #[test]
    fn debug_reports_the_container_name_and_size() {
        let mut r = Resources::new();
        r.insert(Thing(1));

        assert_eq!(
            format!("{r:?}"),
            "Resources { registered: 1, has_parent: false }"
        );
    }
}
