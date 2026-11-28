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

//! The index entry of an asset: what it is, and where its variants are read from.

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use super::asset_uuid::AssetUUID;
use super::{AssetKind, AssetSource};

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

    /// The asset's kind, which selects its decoder; written as its plain name.
    pub kind: AssetKind,

    /// The assets this asset directly depends on, such as a material's
    /// textures.
    pub dependencies: Vec<AssetUUID>,

    /// The engine-ready variants of the asset, keyed by variant name.
    ///
    /// A key names the variant ([`Self::DEFAULT_VARIANT`], `"LOD0"`, `"4K"`);
    /// its value is where that variant's bytes are read from.
    pub variants: HashMap<String, AssetSource>,

    /// Semantic tags that group assets, e.g. all the assets of one level.
    pub tags: Vec<String>,
}

impl AssetMetadata {
    /// The key of the variant every indexed asset has: its file as it is.
    pub const DEFAULT_VARIANT: &'static str = "default";

    /// Returns where the default variant's bytes are read from, if it has one.
    pub fn default_source(&self) -> Option<&AssetSource> {
        self.variants.get(Self::DEFAULT_VARIANT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(variants: HashMap<String, AssetSource>) -> AssetMetadata {
        AssetMetadata {
            uuid: AssetUUID::new_v5("a.png"),
            source_path: PathBuf::from("a.png"),
            kind: crate::asset::kinds::TEXTURE,
            dependencies: Vec::new(),
            variants,
            tags: Vec::new(),
        }
    }

    /// **The default source is the variant keyed `"default"`.**
    #[test]
    fn the_default_source_is_the_variant_keyed_default() {
        let file = AssetSource::Path(PathBuf::from("a.png"));
        let entry = metadata(HashMap::from([("default".to_owned(), file)]));
        assert!(
            matches!(entry.default_source(), Some(AssetSource::Path(p)) if p == &PathBuf::from("a.png"))
        );
        assert_eq!(AssetMetadata::DEFAULT_VARIANT, "default");
    }

    /// **An entry with only other variants has no default source.**
    #[test]
    fn an_entry_without_the_default_variant_has_no_default_source() {
        let file = AssetSource::Path(PathBuf::from("a_4k.png"));
        let entry = metadata(HashMap::from([("4K".to_owned(), file)]));
        assert!(entry.default_source().is_none());
    }
}
