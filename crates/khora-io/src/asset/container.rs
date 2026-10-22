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

//! Sub-assets — assets that live inside another asset's file.
//!
//! A glTF file holds meshes, materials, textures and scenes. Each of them is
//! an asset of its own, with a uuid derived from the file's uuid and a label
//! ([`AssetUUID::sub_asset`]), and an [`khora_core::asset::AssetSource::Sub`] source
//! naming the file that holds it.
//!
//! Two pieces make a file type a container:
//! - a [`SubAssetListerRegistration`], which the index builder runs to list the
//!   sub-assets of a file of that type (reading metadata only);
//! - an [`AssetContainer`], registered on the
//!   [`AssetService`](super::AssetService), which opens the file once and
//!   extracts each sub-asset from it.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use khora_core::asset::AssetUUID;

/// One sub-asset a container file holds, as its lister reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAssetEntry {
    /// The sub-asset's name inside its container — hashed with the
    /// container's uuid into the sub-asset's own uuid.
    pub label: String,
    /// The sub-asset's index type (`"mesh"`, `"material"`, `"texture"`,
    /// `"prefab"`).
    pub type_name: String,
}

/// Inventory entry listing the sub-assets of every file of one index type.
///
/// `type_name` is the **file's** index type (a glTF file stays `"mesh"`); a
/// lister returns an empty list for bytes it does not own (an OBJ file).
///
/// ```ignore
/// inventory::submit! {
///     SubAssetListerRegistration {
///         type_name: "mesh",
///         list: list_gltf_sub_assets,
///     }
/// }
/// ```
pub struct SubAssetListerRegistration {
    /// The index type of the files this lister reads.
    pub type_name: &'static str,
    /// Lists the sub-assets of one file, given its bytes.
    pub list: fn(&[u8]) -> anyhow::Result<Vec<SubAssetEntry>>,
}

inventory::collect!(SubAssetListerRegistration);

/// Whether files of index type `type_name` can hold sub-assets: a lister is
/// registered for the type. Such a file's parts can change with its bytes.
pub fn type_may_hold_sub_assets(type_name: &str) -> bool {
    inventory::iter::<SubAssetListerRegistration>
        .into_iter()
        .any(|lister| lister.type_name == type_name)
}

/// Opens a container file and extracts its sub-assets.
///
/// Registered on the [`AssetService`](super::AssetService) under the
/// container file's index type with
/// [`register_container`](super::AssetService::register_container).
pub trait AssetContainer: Send + Sync + 'static {
    /// Opens the container from its bytes. `source_path` is the container's
    /// path relative to the assets root (for files next to it). The opened
    /// value is cached per container uuid and shared by all its sub-assets.
    fn open(&self, bytes: &[u8], source_path: &Path) -> anyhow::Result<Arc<dyn Any + Send + Sync>>;

    /// Extracts the sub-asset `label` of index type `type_name` from an
    /// opened container, boxed as the concrete asset type for `type_name`.
    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> anyhow::Result<Box<dyn Any + Send>>;
}
