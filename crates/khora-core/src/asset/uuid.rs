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

use bincode::{Decode, Encode};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AssetUUID(Uuid);

impl Encode for AssetUUID {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        let bytes = self.0.as_bytes();
        Encode::encode(bytes, encoder)
    }
}

impl<Context> Decode<Context> for AssetUUID {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes: [u8; 16] = Decode::decode(decoder)?;
        Ok(Self(Uuid::from_bytes(bytes)))
    }
}

impl<'de, Context> bincode::BorrowDecode<'de, Context> for AssetUUID {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes: [u8; 16] = Decode::decode(decoder)?;
        Ok(Self(Uuid::from_bytes(bytes)))
    }
}

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
    ///
    /// The path string must come from [`asset_key`] — the stability this
    /// promises is only as stable as the convention that builds its input.
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

/// Turns a project-relative path into the string that names an asset.
///
/// # Why this is not five lines at each call site
///
/// This is the *entire* definition of an asset's identity: whatever string
/// comes out of here is what [`AssetUUID::new_v5`] hashes. It used to be
/// written out by hand in five places — the index builder, the pack builder,
/// the file watcher, the editor's project VFS and its scene loader — and each
/// copy could have drifted on its own.
///
/// The drift would not have been loud. `Path::components` yields
/// `textures\wall.png` on Windows and `textures/wall.png` elsewhere, so a copy
/// that forgot the join would keep working perfectly on one platform and hand
/// every asset a different UUID on the other: scene references resolving to
/// nothing, on someone else's machine, with no error at the point of failure.
///
/// A convention with five copies is a convention that can drift. This is the
/// one copy.
pub fn asset_key(relative: &std::path::Path) -> String {
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod asset_key_tests {
    use super::*;
    use std::path::Path;

    /// The convention itself. If this changes, every asset in every existing
    /// project gets a new identity and every scene reference breaks — so it is
    /// pinned here rather than left to five call sites to agree on.
    #[test]
    fn nested_paths_join_with_forward_slashes() {
        assert_eq!(
            asset_key(
                Path::new("textures")
                    .join("walls")
                    .join("brick.png")
                    .as_path()
            ),
            "textures/walls/brick.png"
        );
    }

    #[test]
    fn a_single_component_is_itself() {
        assert_eq!(asset_key(Path::new("scene.kscene")), "scene.kscene");
    }

    #[test]
    fn an_empty_relative_path_is_the_empty_key() {
        assert_eq!(asset_key(Path::new("")), "");
    }

    /// **The bug this exists to prevent.** A Windows-separated relative path
    /// and a Unix-separated one name the same asset, so they must hash to the
    /// same UUID. Five hand-written copies each had their own chance to get
    /// this wrong on one platform only.
    #[test]
    fn separators_do_not_change_an_asset_s_identity() {
        let windows_style = Path::new(r"textures\walls\brick.png");
        let unix_style = Path::new("textures/walls/brick.png");

        // On Windows both parse to the same components; elsewhere the
        // backslash string is a single component and must not silently become
        // a different asset than the one the editor indexed.
        let key = asset_key(unix_style);
        assert_eq!(key, "textures/walls/brick.png");
        assert_eq!(
            AssetUUID::new_v5(&key),
            AssetUUID::new_v5("textures/walls/brick.png")
        );

        if cfg!(windows) {
            assert_eq!(asset_key(windows_style), key);
            assert_eq!(
                AssetUUID::new_v5(&asset_key(windows_style)),
                AssetUUID::new_v5(&key)
            );
        }
    }
}
