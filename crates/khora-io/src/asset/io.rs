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

/// Trait for asset I/O backends (file system or pack archive).
///
/// Implementations handle the low-level reading of raw bytes from storage.
/// The `AssetService` dispatches to this trait based on the `AssetSource` variant.
pub trait AssetIo: Send + Sync {
    /// Loads raw bytes from the given asset source.
    fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>>;
}
