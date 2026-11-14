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

//! The type-erased store of one asset type's cached handles: what lets the
//! service drop a uuid from every typed storage without knowing their types.

use std::any::Any;

use khora_core::asset::{Asset, AssetUUID};
use khora_data::assets::Assets;

/// The type-erased view of an `Assets<A>`, implemented for every asset type.
pub(in crate::asset) trait AnyAssets: Any + Send + Sync {
    /// Removes the cached handle for `uuid`; returns whether there was one.
    fn remove_uuid(&mut self, uuid: &AssetUUID) -> bool;

    /// Returns the storage as `Any`, to downcast it back to its `Assets<A>`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<A: Asset + Send + Sync + 'static> AnyAssets for Assets<A> {
    fn remove_uuid(&mut self, uuid: &AssetUUID) -> bool {
        self.remove(uuid).is_some()
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
