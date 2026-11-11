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
use std::path::Path;
use std::sync::Arc;

use crate::trust::{FileGuard, Refusals, Trust};

/// Resolves external resources (buffers, images) referenced by a URI within
/// an asset file.
pub trait GltfResourceResolver: Send + Sync {
    /// Resolves an external buffer URI to its binary data.
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;

    /// Resolves an external image URI to its binary data.
    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;
}

/// Resolves resources from the local filesystem relative to a base path,
/// through a [`FileGuard`]: a URI leaving the base — by `..`, as an absolute
/// path, or through a link — is refused, as is a file over its cap.
pub struct FileSystemResolver {
    guard: FileGuard,
}

impl FileSystemResolver {
    /// A resolver reading under `base_path` as a trusted project's files,
    /// its refusals kept in memory. A run resolves through
    /// [`Self::guarded`].
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self::guarded(FileGuard::new(
            base_path.as_ref(),
            Trust::Project,
            Arc::new(Refusals::new(None, None)),
        ))
    }

    /// A resolver reading every file through `guard`.
    pub fn guarded(guard: FileGuard) -> Self {
        Self { guard }
    }

    fn read(&self, what: &str, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.guard.read(uri).map_err(|error| {
            format!(
                "Failed to read external {what} '{uri}' under {}: {error}",
                self.guard.root().display()
            )
            .into()
        })
    }
}

impl GltfResourceResolver for FileSystemResolver {
    fn resolve_buffer(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.read("buffer", uri)
    }

    fn resolve_image(&self, uri: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.read("image", uri)
    }
}

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
