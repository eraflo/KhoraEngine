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

//! [`HandleComponent`], the component that attaches a loaded asset to an entity.

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use std::ops::Deref;

use crate::ecs::Component;

/// A loaded asset attached to an entity: a shared handle and the asset's UUID.
///
/// Derefs to the asset itself; cloning shares the data and needs no `T: Clone`.
pub struct HandleComponent<T: Asset> {
    /// The shared, reference-counted asset data.
    pub handle: AssetHandle<T>,
    /// The asset's stable identifier.
    pub uuid: AssetUUID,
}

impl<T: Asset> Component for HandleComponent<T> {}

// Written by hand: a derive would require `T: Clone`.
impl<T: Asset> Clone for HandleComponent<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            uuid: self.uuid,
        }
    }
}

impl<T: Asset> Deref for HandleComponent<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}
