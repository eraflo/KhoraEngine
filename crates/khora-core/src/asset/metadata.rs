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

/// Serializable metadata that describes an asset and its relationships.
///
/// This structure serves as the "identity card" for each asset within the engine's
/// Virtual File System (VFS). It contains all the information needed by the
/// `AssetAgent` to make intelligent loading and management decisions *without*
/// having to load the actual asset data from disk.
///
/// A collection of these metadata entries forms the VFS "Index".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetadata {
    /// The unique, stable identifier for this asset. This is the primary key.
    pub uuid: AssetUUID,

    /// The canonical path to the original source file (e.g., a `.blend` or `.png`).
    /// This is primarily used by the asset importer and editor tooling.
    pub source_path: PathBuf,

    /// A string identifier for the asset's type (e.g., "texture", "mesh", "material").
    /// This is used by the loading system to select the correct `AssetLoader` trait object.
    pub asset_type_name: String,

    /// A list of other assets that this asset directly depends on.
    /// For example, a material asset would list its required texture assets here.
    /// This information is crucial for tracking dependencies and ensuring all
    /// necessary assets are loaded.
    pub dependencies: Vec<AssetUUID>,

    /// A map of available, pre-processed asset variants ready for runtime use.
    ///
    /// The key is a variant identifier (e.g., "LOD0", "4K", "low_quality"),
    /// and the value is the path to the compiled, engine-ready file for that variant.
    /// This map allows the `AssetAgent` to make strategic choices, such as loading
    /// a lower-quality texture to stay within a VRAM budget.
    pub variants: HashMap<String, PathBuf>,

    /// A collection of semantic tags for advanced querying and organization.
    /// Tags can be used to group assets for collective operations, such as
    /// loading all assets for a specific game level or character.
    pub tags: Vec<String>,
}
