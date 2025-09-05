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

//! Defines a generic component for attaching asset handles to entities.

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use std::ops::Deref;

use crate::ecs::Component;

/// A generic ECS component that associates an entity with a shared asset resource.
/// It holds both a handle to the loaded data and the asset's unique identifier.
pub struct HandleComponent<T: Asset> {
    /// A shared, reference-counted pointer to the asset's data.
    pub handle: AssetHandle<T>,
    /// The unique, persistent identifier of the asset.
    pub uuid: AssetUUID,
}

/// Implement the `Component` trait to make `HandleComponent` usable by the ECS.
impl<T: Asset> Component for HandleComponent<T> {}

/// Manual implementation of `Clone`.
/// This is cheap as it only clones the reference-counted pointer and the UUID.
/// It correctly does NOT require `T` to be `Clone`.
impl<T: Asset> Clone for HandleComponent<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            uuid: self.uuid, // AssetUUID is Copy
        }
    }
}

/// Implement `Deref` to the inner `AssetHandle` for ergonomic access to the asset data.
/// This allows calling methods of `T` directly on the component (e.g., `my_mesh_component.vertex_size()`).
impl<T: Asset> Deref for HandleComponent<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}
