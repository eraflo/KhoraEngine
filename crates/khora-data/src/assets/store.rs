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

//! The engine-wide store of every asset cache (`Assets<GpuMesh>`,
//! `Assets<GpuMaterial>`, …), one typed sub-store per asset type, created on
//! first access.

use crate::assets::Assets;
use khora_core::asset::Asset;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Shared, type-erased registry of `Assets<T>` sub-stores keyed by `TypeId`.
///
/// Cloning is cheap (clones an `Arc`); all clones share the same underlying
/// sub-stores.
#[derive(Clone, Default)]
pub struct AssetStore {
    inner: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl AssetStore {
    /// Creates a new, empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the shared `Assets<T>` sub-store, creating it on first access.
    ///
    /// Every caller gets the same sub-store for a given `T`; it may be held
    /// across frames.
    ///
    /// # Panics
    ///
    /// If a thread panicked while holding the store's lock.
    pub fn store<T: Asset>(&self) -> Arc<RwLock<Assets<T>>> {
        let mut map = self.inner.lock().expect("AssetStore mutex poisoned");
        let entry = map
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Arc::new(RwLock::new(Assets::<T>::new()))));
        entry
            .downcast_ref::<Arc<RwLock<Assets<T>>>>()
            .expect("AssetStore type mismatch for TypeId")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::AssetUUID;

    #[derive(Debug, PartialEq)]
    struct Foo(u32);
    impl Asset for Foo {}
    struct Bar(#[allow(dead_code)] &'static str);
    impl Asset for Bar {}

    #[test]
    fn same_type_returns_shared_substore() {
        let store = AssetStore::new();
        let uuid = AssetUUID::new();
        store
            .store::<Foo>()
            .write()
            .unwrap()
            .insert(uuid, khora_core::asset::AssetHandle::new(Foo(7)));
        // A second handle sees the same data.
        let got = store.store::<Foo>();
        let guard = got.read().unwrap();
        assert_eq!(guard.get(&uuid).map(|h| h.0), Some(7));
    }

    #[test]
    fn distinct_types_are_independent() {
        let store = AssetStore::new();
        let foo_uuid = AssetUUID::new();
        store
            .store::<Foo>()
            .write()
            .unwrap()
            .insert(foo_uuid, khora_core::asset::AssetHandle::new(Foo(1)));
        // Bar sub-store is independent and empty.
        assert!(store
            .store::<Bar>()
            .read()
            .unwrap()
            .get(&foo_uuid)
            .is_none());
    }

    #[test]
    fn clone_shares_backing() {
        let store = AssetStore::new();
        let clone = store.clone();
        let uuid = AssetUUID::new();
        store
            .store::<Bar>()
            .write()
            .unwrap()
            .insert(uuid, khora_core::asset::AssetHandle::new(Bar("x")));
        assert!(clone.store::<Bar>().read().unwrap().get(&uuid).is_some());
    }
}
