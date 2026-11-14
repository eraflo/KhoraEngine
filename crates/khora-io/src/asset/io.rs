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

pub use super::asset_writer::AssetWriter;

/// Trait for asset I/O backends (file system or pack archive).
///
/// Implementations handle the low-level reading of raw bytes from storage.
/// The `AssetService` dispatches to this trait based on the `AssetSource` variant.
pub trait AssetIo: Send + Sync {
    /// Loads raw bytes from the given asset source.
    ///
    /// Takes `&self`: one backend serves reads from several threads at once,
    /// each read returning exactly its own source's bytes. A backend holds no
    /// cursor or other per-read state shared between calls.
    fn load_bytes(&self, source: &AssetSource) -> Result<Vec<u8>>;

    /// The file `source` is read from, when it is a file of its own: what a
    /// refusal of its bytes names it by. `None` for a pack entry, a
    /// sub-asset, or a backend with no files.
    fn locate(&self, source: &AssetSource) -> Option<std::path::PathBuf> {
        let _ = source;
        None
    }
}
