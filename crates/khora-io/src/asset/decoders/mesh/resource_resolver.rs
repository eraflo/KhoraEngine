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

//! External-resource resolution for complex asset formats like GLTF.

use std::error::Error;

pub use super::file_system_resolver::FileSystemResolver;
pub use super::no_op_resource_resolver::NoOpResourceResolver;

/// Resolves external resources (buffers, images) referenced by a URI within
/// an asset file.
pub trait GltfResourceResolver: Send + Sync {
    /// Resolves an external buffer URI to its binary data.
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;

    /// Resolves an external image URI to its binary data.
    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;
}
