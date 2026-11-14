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

//! The `SubAssetListerRegistration` inventory entry: what lists the
//! sub-assets a container file holds, without opening it.

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
