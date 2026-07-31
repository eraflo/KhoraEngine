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
use std::path::{Path, PathBuf};

use super::{AssetIo, AssetWriter};

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

    /// Returns the root directory the loader reads/writes from.
    pub fn root(&self) -> &Path {
        &self.root
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

impl AssetWriter for FileLoader {
    fn write_bytes(&self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        let full_path = self.root.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory for {:?}", full_path)
            })?;
        }
        std::fs::write(&full_path, bytes)
            .with_context(|| format!("Failed to write asset: {:?}", full_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_bytes_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        loader
            .write_bytes(Path::new("scenes/default.kscene"), b"SCN")
            .unwrap();
        let written = std::fs::read(dir.path().join("scenes").join("default.kscene")).unwrap();
        assert_eq!(written, b"SCN");
    }

    #[test]
    fn round_trip_via_load_bytes() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        loader
            .write_bytes(Path::new("textures/foo.png"), b"PNG")
            .unwrap();

        let mut reader = FileLoader::new(dir.path());
        let bytes = reader
            .load_bytes(&AssetSource::Path(PathBuf::from("textures/foo.png")))
            .unwrap();
        assert_eq!(bytes, b"PNG");
    }
}
