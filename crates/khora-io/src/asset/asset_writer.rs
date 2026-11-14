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

//! Editor-time writing of assets back to storage.

use anyhow::Result;
use std::path::Path;

/// Writes assets back to storage, beside [`AssetIo`](crate::asset::AssetIo).
///
/// A separate trait because a pack ([`PackLoader`](crate::asset::PackLoader)) is
/// read-only: only [`FileLoader`](crate::asset::FileLoader) implements it.
pub trait AssetWriter: Send + Sync {
    /// Writes `bytes` at `rel_path`, relative to the loader's root.
    ///
    /// Creates the missing directories. The path has the shape an
    /// `AssetSource::Path` records, so the next index build lists the file.
    fn write_bytes(&self, rel_path: &Path, bytes: &[u8]) -> Result<()>;
}
