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

//! The on-disk shape of `asset-registry.ron`.

use khora_core::asset::AssetUUID;
use serde::{Deserialize, Serialize};

/// One persisted `path → uuid` binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RegistryEntry {
    /// Frozen identity of the asset (serialized as a hyphenated UUID string).
    pub(super) uuid: AssetUUID,
    /// Forward-slash relative path under `assets/`.
    pub(super) path: String,
}

/// On-disk shape of `asset-registry.ron`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct RegistryFile {
    /// Bindings, sorted by UUID for deterministic, merge-friendly output.
    pub(super) entries: Vec<RegistryEntry>,
}
