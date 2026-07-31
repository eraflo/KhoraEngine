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

/// On-disk compression scheme for a packed asset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionKind {
    /// Raw bytes — no compression applied. Fastest read path.
    #[default]
    None,
    /// LZ4 block compression (`lz4_flex`). Cheap to decompress, modest
    /// ratio. Good default for shipping builds.
    Lz4,
}

/// Represents the physical source of an asset's data.
///
/// This enum allows the asset system to transparently handle assets from two
/// different contexts:
/// - In editor/development mode, assets are loaded directly from loose files on disk (`Path`).
/// - In release/standalone mode, assets are loaded from an optimized packfile (`Packed`).
///
/// **Pack format version:** v2 added per-entry compression (`compression`,
/// `uncompressed_size`). v1 packs (without these fields) are no longer
/// supported on the read side; the loader bails on the header version
/// check before reaching this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetSource {
    /// The asset is a loose file on disk. The `PathBuf` points to the file.
    Path(PathBuf),
    /// The asset is located within a packfile.
    Packed {
        /// Byte offset from the start of the asset region (i.e. *after*
        /// the 24-byte header) in `data.pack`.
        offset: u64,
        /// Number of bytes to read from the pack at `offset` — the
        /// **on-disk size**, possibly compressed.
        size: u64,
        /// Size of the asset after decompression. Equals `size` when
        /// `compression == CompressionKind::None`.
        uncompressed_size: u64,
        /// Compression scheme applied to the bytes between `offset` and
        /// `offset + size`.
        compression: CompressionKind,
    },
}

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
    /// and the value is the source of the compiled, engine-ready file for that variant. (Which contains the necessary metadata for loading the asset.)
    /// This map allows the `AssetAgent` to make strategic choices, such as loading
    /// a lower-quality texture to stay within a VRAM budget.
    pub variants: HashMap<String, AssetSource>,

    /// A collection of semantic tags for advanced querying and organization.
    /// Tags can be used to group assets for collective operations, such as
    /// loading all assets for a specific game level or character.
    pub tags: Vec<String>,
}
