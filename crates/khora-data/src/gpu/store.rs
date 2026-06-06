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

//! Unified, engine-wide GPU/asset resource store.
//!
//! [`AssetStore`] is the single home for every projected asset cache —
//! `Assets<GpuMesh>`, `Assets<GpuMaterial>`, `Assets<CpuTexture>`, and any
//! future type. It replaces the per-type named wrappers (`GpuCache`,
//! `GpuMaterialCache`, `CpuTextureCache`) that each existed only to get a
//! distinct `TypeId` in the runtime resource registry.
//!
//! It is the **Data** side of CLAD: a store of resources projected from the
//! ECS, consumed by the render descent. One `AssetStore` is created at
//! bootstrap, registered into `runtime.resources`, and shared (cheap `Arc`
//! clone) by the projection and every consumer. Adding a new asset type
//! needs zero new wiring: `store.store::<T>()` lazily creates the typed
//! sub-store on first access.

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
    /// The returned `Arc<RwLock<Assets<T>>>` can be held across frames; it is
    /// the same handle every caller receives for a given `T`.
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
    struct Bar(&'static str);
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
