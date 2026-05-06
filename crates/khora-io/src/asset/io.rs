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

//! Abstraction over asset I/O backends.

use anyhow::Result;
use khora_core::asset::AssetSource;
use std::path::Path;

/// Trait for asset I/O backends (file system or pack archive).
///
/// Implementations handle the low-level reading of raw bytes from storage.
/// The `AssetService` dispatches to this trait based on the `AssetSource` variant.
pub trait AssetIo: Send + Sync {
    /// Loads raw bytes from the given asset source.
    fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>>;
}

/// Sibling trait to [`AssetIo`] for editor-time *writing* of assets back to
/// storage. Implemented only by [`crate::asset::FileLoader`] — release builds
/// (`PackLoader`) are intentionally read-only, which is why `AssetWriter`
/// lives separately rather than extending `AssetIo`.
///
/// Used by the editor's `ProjectVfs` to persist scene saves and any other
/// asset mutation through the same root path the FileLoader reads from.
pub trait AssetWriter: Send + Sync {
    /// Writes `bytes` to a path **relative to the loader's root**. Creates
    /// intermediate directories as needed. The relative path is the same
    /// shape that `AssetSource::Path(rel)` records — `IndexBuilder` will
    /// see the new file on the next scan and assign it a stable UUID.
    fn write_bytes(&self, rel_path: &Path, bytes: &[u8]) -> Result<()>;
}
