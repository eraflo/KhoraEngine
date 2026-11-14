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

//! Mesh decoders (glTF, OBJ), the dispatcher that picks one by the file's bytes,
//! and the resolvers a glTF reads its external files through.

mod dispatcher;
mod file_system_resolver;
mod gltf;
mod gltf_checks;
mod gltf_container;
mod gltf_resources;
mod gltf_scene;
mod no_op_resource_resolver;
mod obj;
mod packed_resolver;
mod resource_resolver;

pub use dispatcher::MeshDispatcher;
pub use gltf::GltfDecoder;
pub use gltf_container::{list_gltf_sub_assets, GltfContainer};
pub use obj::ObjDecoder;
pub use packed_resolver::PackedResolver;
pub use resource_resolver::*;

#[cfg(test)]
mod tests;
