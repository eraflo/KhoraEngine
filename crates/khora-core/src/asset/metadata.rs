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

use super::asset_uuid::AssetUUID;
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

/// The physical source of an asset's data.
///
/// A loose file on disk in development (`Path`), a span of the packfile in a
/// shipped build (`Packed`), or a part of another asset (`Sub`). See the book:
/// concepts/assets.md, "Dev and packed are the same path".
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
    /// The asset lives inside another asset: a mesh, material, texture or
    /// scene of a glTF file.
    ///
    /// It has no bytes of its own: they are read from `container`'s own source
    /// and the asset is extracted by `label`.
    Sub {
        /// The uuid of the asset that holds this one.
        container: AssetUUID,
        /// The name of this asset inside its container.
        label: String,
    },
}

/// Serializable metadata that describes an asset and its relationships.
///
/// It tells the asset service how to load an asset without reading its data.
/// The index of the VFS is a collection of these entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetadata {
    /// The asset's stable identifier: the key of this entry.
    pub uuid: AssetUUID,

    /// The path to the original source file (e.g. a `.gltf` or `.png`), for
    /// the importer and editor tooling.
    pub source_path: PathBuf,

    /// The asset's type name (e.g. `"texture"`, `"mesh"`, `"material"`), which
    /// selects its decoder.
    pub asset_type_name: String,

    /// The assets this asset directly depends on, such as a material's
    /// textures.
    pub dependencies: Vec<AssetUUID>,

    /// The engine-ready variants of the asset, keyed by variant name.
    ///
    /// A key names the variant (e.g. `"default"`, `"LOD0"`, `"4K"`); its value
    /// is where that variant's bytes are read from.
    pub variants: HashMap<String, AssetSource>,

    /// Semantic tags that group assets, e.g. all the assets of one level.
    pub tags: Vec<String>,
}
