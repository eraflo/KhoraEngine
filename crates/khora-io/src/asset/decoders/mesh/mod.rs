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

//! Mesh decoders for various formats (GLTF, OBJ) plus a sniffing dispatcher.
//!
//! Unlike texture / font, the mesh slot has multiple competing implementations
//! — the consumer (editor / runtime) explicitly picks one when constructing
//! the [`crate::asset::AssetService`]. Default choice is [`MeshDispatcher`],
//! which delegates to gltf or obj based on byte sniffing.
//!
//! ```ignore
//! use khora_io::asset::{MeshDispatcher, FileSystemResolver};
//! use std::sync::Arc;
//! let resolver = Arc::new(FileSystemResolver::new(project_root.join("assets/meshes")));
//! svc.register_decoder::<Mesh>("mesh", MeshDispatcher::new(resolver));
//! ```

mod dispatcher;
mod gltf;
mod obj;
mod resource_resolver;

pub use dispatcher::MeshDispatcher;
pub use gltf::GltfDecoder;
pub use obj::ObjDecoder;
pub use resource_resolver::*;
