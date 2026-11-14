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

//! A glTF resource resolver that refuses every external resource.

use std::error::Error;

use super::resource_resolver::GltfResourceResolver;

/// Resolver that fails on every external URI lookup. Suitable as a default
/// for self-contained `.glb` files (which embed all buffers + images) and
/// as a placeholder when no project context is available.
///
/// `MeshDispatcher::default` uses this. For `.gltf` files referencing
/// external `.bin` / texture buffers, construct
/// [`MeshDispatcher::new(Arc::new(FileSystemResolver::new(...)))`] instead.
pub struct NoOpResourceResolver;

impl GltfResourceResolver for NoOpResourceResolver {
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        Err(format!(
            "NoOpResourceResolver cannot fetch external buffer '{}'; \
             use FileSystemResolver for .gltf files with external buffers.",
            uri
        )
        .into())
    }
    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        Err(format!(
            "NoOpResourceResolver cannot fetch external image '{}'; \
             use FileSystemResolver for .gltf files with external textures.",
            uri
        )
        .into())
    }
}
