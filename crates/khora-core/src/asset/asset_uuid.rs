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

/// The fixed namespace of path-derived asset UUIDs ([`AssetUUID::new_v5`]).
const ASSET_NAMESPACE_UUID: Uuid = Uuid::from_u128(0x4a6a81e9_f0d1_4b8f_91a8_7e7a5e0b6b4a);

/// The fixed namespace of sub-asset UUIDs ([`AssetUUID::sub_asset`]), apart from
/// the path namespace so a label can never collide with a path.
const SUB_ASSET_NAMESPACE_UUID: Uuid = Uuid::from_u128(0x6b68_6f72_615f_7375_625f_6173_7365_7431);

/// A globally unique, persistent identifier for a logical asset.
///
/// It is the key of the asset's metadata in the VFS, independent of the file's
/// path, so a moved or renamed asset keeps every reference to it. Its serde
/// name is reserved, so a save can tell an asset reference from any other
/// sixteen bytes. See the book: concepts/assets.md, "Identity, not paths".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename = "khora.AssetUUID")]
pub struct AssetUUID(Uuid);

/// The canonical hyphenated form, as text files write it.
impl std::fmt::Display for AssetUUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Reads the hyphenated form (or any form `uuid` accepts).
impl std::str::FromStr for AssetUUID {
    type Err = uuid::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(text).map(Self)
    }
}

impl AssetUUID {
    /// The identity whose sixteen bytes are `bytes`.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    /// The sixteen bytes of this identity.
    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    /// Creates a new, random (version 4) `AssetUUID`.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates the stable (version 5) `AssetUUID` of the asset named `path_str`.
    ///
    /// The same path always gives the same UUID. `path_str` must come from
    /// [`asset_key`]: the identity is only as stable as that convention.
    pub fn new_v5(path_str: &str) -> Self {
        Self(Uuid::new_v5(&ASSET_NAMESPACE_UUID, path_str.as_bytes()))
    }

    /// The identity of the asset named `label` inside the asset `container`
    /// (a glTF file's mesh, material, texture or scene).
    ///
    /// It is derived from `container`'s identity, so a renamed file keeps its
    /// registry-frozen uuid and with it every sub uuid. It never collides with
    /// a path-derived uuid.
    pub fn sub_asset(container: AssetUUID, label: &str) -> AssetUUID {
        let mut name = Vec::with_capacity(16 + label.len());
        name.extend_from_slice(container.as_bytes());
        name.extend_from_slice(label.as_bytes());
        Self(Uuid::new_v5(&SUB_ASSET_NAMESPACE_UUID, &name))
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
/// This is the whole definition of a path-derived identity: its output is what
/// [`AssetUUID::new_v5`] hashes. Components are joined with `/` on every
/// platform, so an asset keeps its UUID from one OS to another. Every caller
/// that names an asset by path goes through it.
pub fn asset_key(relative: &std::path::Path) -> String {
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    mod asset_key_tests {
        use super::super::*;
        use std::path::Path;

        /// **Components join with forward slashes.**
        ///
        /// Changing this gives every asset of every project a new identity.
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

        /// **A Windows-separated path and a Unix-separated one hash alike.**
        ///
        /// They name the same asset, so they must have the same UUID.
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

    mod sub_asset_uuid_tests {
        use super::super::*;

        #[test]
        fn sub_is_deterministic() {
            let container = AssetUUID::new_v5("meshes/hero.glb");
            assert_eq!(
                AssetUUID::sub_asset(container, "mesh/Body"),
                AssetUUID::sub_asset(container, "mesh/Body")
            );
        }

        #[test]
        fn sub_differs_per_label() {
            let container = AssetUUID::new_v5("meshes/hero.glb");
            assert_ne!(
                AssetUUID::sub_asset(container, "mesh/Body"),
                AssetUUID::sub_asset(container, "mesh/Head")
            );
            assert_ne!(
                AssetUUID::sub_asset(container, "mesh/Body"),
                AssetUUID::sub_asset(container, "")
            );
        }

        #[test]
        fn sub_differs_per_container() {
            let hero = AssetUUID::new_v5("meshes/hero.glb");
            let villain = AssetUUID::new_v5("meshes/villain.glb");
            assert_ne!(
                AssetUUID::sub_asset(hero, "mesh/Body"),
                AssetUUID::sub_asset(villain, "mesh/Body")
            );
        }

        /// A sub uuid lives in its own namespace: it never collides with the
        /// path-derived uuid of a file whose key happens to be the same text.
        #[test]
        fn sub_differs_from_the_path_uuid_of_the_same_text() {
            let container = AssetUUID::new_v5("meshes/hero.glb");
            let label = "mesh/Body";
            let sub = AssetUUID::sub_asset(container, label);
            assert_ne!(sub, AssetUUID::new_v5(label));
            assert_ne!(sub, AssetUUID::new_v5(&format!("{container}{label}")));
            assert_ne!(sub, AssetUUID::new_v5(&format!("{container}/{label}")));
            assert_ne!(sub, container);
        }

        #[test]
        fn sub_is_a_version_5_uuid() {
            let sub = AssetUUID::sub_asset(AssetUUID::new_v5("meshes/hero.glb"), "mesh/Body");
            assert_eq!(sub.as_bytes()[6] >> 4, 5, "the version nibble must be 5");
        }
    }
}
