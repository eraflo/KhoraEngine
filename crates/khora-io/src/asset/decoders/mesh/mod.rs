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

//! Mesh decoders for various formats (GLTF, OBJ).

mod gltf;
mod obj;
mod resource_resolver;

pub use gltf::GltfDecoder;
pub use obj::ObjDecoder;
pub use resource_resolver::*;

use khora_core::renderer::api::scene::Mesh;

use crate::asset::AssetDecoder;

/// Marker trait for decoders producing `Mesh` assets.
pub trait MeshDecoder: AssetDecoder<Mesh> + Send + Sync + 'static {}

impl<T> MeshDecoder for T where T: AssetDecoder<Mesh> + Send + Sync + 'static {}
