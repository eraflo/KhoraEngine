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

use super::uuid::AssetUUID;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

/// Serializable metadata that describes an asset and its relationship to other assets.
///
/// This structure contains all the information the Virtual File System (VFS)
/// and the `AssetAgent` need to manage, load, and adapt asset usage without
/// having to load the actual asset data from disk. It serves as the "identity card"
/// for each asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetadata {
    /// The unique, stable identifier for this asset.
    pub uuid: AssetUUID,

    /// The path to the original source file (e.g., a `.blend` or `.png` file).
    /// This is primarily used by the asset importer in the editor.
    pub source_path: PathBuf,

    /// A string identifier for the asset's type (e.g., "texture", "mesh").
    /// This is used by the `AssetAgent` to select the correct `AssetLoader`.
    pub asset_type_name: String,

    /// A list of other assets that this asset depends on.
    /// For example, a material asset would list its texture assets here.
    pub dependencies: Vec<AssetUUID>,

    /// A map of available asset variants.
    /// The key is a variant identifier (e.g., "LOD0", "4K", "low_quality"),
    /// and the value is the path to the compiled file for that variant.
    pub variants: HashMap<String, PathBuf>,

    /// A collection of semantic tags for advanced querying and organization.
    pub tags: Vec<String>,
}