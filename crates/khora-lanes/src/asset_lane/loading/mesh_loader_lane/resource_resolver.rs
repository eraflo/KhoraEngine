// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Defines the abstraction for resolving external resources for complex asset formats like GLTF.

use std::error::Error;
use std::path::{Path, PathBuf};

/// A trait for resolving external resources (like buffers or images) referenced by a URI
/// within an asset file.
pub trait GltfResourceResolver: Send + Sync {
    /// Resolves an external buffer URI to its binary data.
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;

    /// Resolves an external image URI to its binary data.
    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;
}

/// A default implementation of `GltfResourceResolver` that resolves resources
/// from the local filesystem relative to a given base path.
pub struct FileSystemResolver {
    base_path: PathBuf,
}

impl FileSystemResolver {
    /// Creates a new `FileSystemResolver` with a specified base path.
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }
}

impl GltfResourceResolver for FileSystemResolver {
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let path = self.base_path.join(uri);
        std::fs::read(&path)
            .map_err(|e| format!("Failed to read external buffer from '{:?}': {}", path, e).into())
    }

    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let path = self.base_path.join(uri);
        std::fs::read(&path)
            .map_err(|e| format!("Failed to read external image from '{:?}': {}", path, e).into())
    }
}
