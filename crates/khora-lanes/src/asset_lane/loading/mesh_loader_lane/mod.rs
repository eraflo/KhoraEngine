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

//! Defines lanes for loading mesh assets.

mod gltf_loader_lane;
mod obj_loader_lane;
mod resource_resolver;

pub use gltf_loader_lane::*;
pub use obj_loader_lane::*;
pub use resource_resolver::*;

use super::AssetLoaderLane;
use khora_core::renderer::api::scene::Mesh;

/// Common trait for all mesh loaders
pub trait MeshLoaderLane: AssetLoaderLane<Mesh> + Send + Sync + 'static {}

// Implement the trait for all types that implement AssetLoaderLane<Mesh>
impl<T> MeshLoaderLane for T where T: AssetLoaderLane<Mesh> + Send + Sync + 'static {}
