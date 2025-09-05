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

//! Defines a component for attaching a generic material handle to an entity.

use crate::ecs::Component;
use khora_core::asset::{AssetHandle, Material, AssetUUID};

/// A component that attaches any type of material to an entity.
///
/// It uses a trait object (`Box<dyn Material>`) to store a handle to any
/// concrete material type, allowing the ECS to remain agnostic to the specific
/// material implementations.
#[derive(Clone)]
pub struct MaterialComponent {
    /// A shared handle to the type-erased material data.
    pub handle: AssetHandle<Box<dyn Material>>,
    /// The unique identifier of the material asset.
    pub uuid: AssetUUID,
}

impl Component for MaterialComponent {}