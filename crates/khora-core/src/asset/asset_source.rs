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

//! Where an asset's bytes are read from.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{AssetUUID, CompressionKind};

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
