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
}

impl Default for AssetUUID {
    /// Creates a new, random (version 4) `AssetUUID`.
    fn default() -> Self {
        Self::new()
    }
}
