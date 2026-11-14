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

/// A resolver refusing every external URI: enough for a `.glb`, which embeds them.
///
/// [`MeshDispatcher::default`](super::MeshDispatcher::default) uses it. A `.gltf`
/// with external buffers or textures needs a [`FileSystemResolver`](super::FileSystemResolver).
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
