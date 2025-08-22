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

/// A unique, stable identifier for an asset.
///
/// This UUID serves as the primary key for all assets within the engine's
/// Virtual File System (VFS). It decouples the asset's identity from its
/// file path, allowing for flexible storage and management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetUUID(Uuid);

impl AssetUUID {
    /// Creates a new, unique AssetUUID (version 4).
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AssetUUID {
    /// Creates a new, unique AssetUUID.
    fn default() -> Self {
        Self::new()
    }
}