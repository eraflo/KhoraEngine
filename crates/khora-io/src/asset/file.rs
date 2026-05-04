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

//! File-based asset loader for editor/development mode.
//!
//! Reads assets directly from individual files on disk under a root directory.

use anyhow::{bail, Context, Result};
use khora_core::asset::AssetSource;
use std::path::PathBuf;

use super::AssetIo;

/// File-based asset loader for editor/development mode.
///
/// Reads assets directly from individual files on disk. The `root` path is
/// typically `<project>/assets/`.
pub struct FileLoader {
    root: PathBuf,
}

impl FileLoader {
    /// Creates a new `FileLoader` with the given root directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl AssetIo for FileLoader {
    fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Path(rel) => {
                let full_path = self.root.join(rel);
                std::fs::read(&full_path)
                    .with_context(|| format!("Failed to read asset: {:?}", full_path))
            }
            AssetSource::Packed { .. } => {
                bail!("FileLoader does not support Packed sources")
            }
        }
    }
}
