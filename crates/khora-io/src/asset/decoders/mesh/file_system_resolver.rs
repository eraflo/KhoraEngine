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

//! Resolves a glTF's external resources from the local filesystem, under a guard.

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use super::resource_resolver::GltfResourceResolver;
use crate::trust::{FileGuard, Refusals, Trust};

/// Resolves a glTF's external files under a base path, through a [`FileGuard`].
///
/// A URI leaving the base (by `..`, as an absolute path or through a link) is
/// refused, as is a file over its cap.
pub struct FileSystemResolver {
    guard: FileGuard,
}

impl FileSystemResolver {
    /// Creates a resolver reading under `base_path` as a trusted project's files.
    ///
    /// Its refusals stay in memory; a running project resolves through [`Self::guarded`].
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self::guarded(FileGuard::new(
            base_path.as_ref(),
            Trust::Project,
            Arc::new(Refusals::new(None, None)),
        ))
    }

    /// Creates a resolver reading every file through `guard`.
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
