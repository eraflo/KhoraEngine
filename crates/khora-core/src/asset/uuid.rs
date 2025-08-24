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

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A constant, randomly generated namespace for our asset UUIDs.
/// This ensures that UUIDs generated from the same path are always the same.
const ASSET_NAMESPACE_UUID: Uuid = Uuid::from_u128(0x4a6a81e9_f0d1_4b8f_91a8_7e7a5e0b6b4a);

/// A globally unique, persistent identifier for a logical asset.
///
/// This UUID represents the "idea" of an asset, completely decoupled from its
/// physical file path. It is the primary key used by the Virtual File System (VFS)
/// to track and retrieve asset metadata.
///
/// By using a stable UUID, assets can be moved, renamed, or have their source
/// data modified without breaking references to them in scenes or other assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetUUID(Uuid);

impl AssetUUID {
    /// Creates a new, random (version 4) `AssetUUID`.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a new, stable AssetUUID (version 5) from a given path.
    ///
    /// This is the preferred method for generating UUIDs for assets on disk,
    /// as it guarantees that the UUID will be the same every time the asset
    /// pipeline is run for the same file.
    pub fn new_v5(path_str: &str) -> Self {
        Self(Uuid::new_v5(&ASSET_NAMESPACE_UUID, path_str.as_bytes()))
    }
}

impl Default for AssetUUID {
    /// Creates a new, random (version 4) `AssetUUID`.
    fn default() -> Self {
        Self::new()
    }
}
