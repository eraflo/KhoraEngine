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

//! Asset dependency extraction for the index builder.
//!
//! The index builder records, for each asset, the UUIDs of the other assets it
//! directly references (a material's textures, a scene's prefabs, …). Those
//! references populate [`AssetMetadata::dependencies`], which the asset agent
//! uses to load an asset's prerequisites without first decoding it.
//!
//! [`extract_dependencies`] is the single dispatch seam: it switches on the
//! canonical asset type name (the same names [`asset_type_for_extension`]
//! produces) and delegates to a per-format extractor. A type with no extractor
//! returns an empty list — that is the historical behaviour and the extension
//! point: supporting a new format means adding one match arm, not rewiring the
//! builder.
//!
//! # Determinism
//!
//! Extracted UUID lists are deduplicated and sorted. The index builder's
//! contract is byte-determinism (two builds of the same project produce a
//! byte-identical index); unsorted or duplicated dependencies would break it.
//!
//! [`AssetMetadata::dependencies`]: khora_core::asset::AssetMetadata::dependencies
//! [`asset_type_for_extension`]: crate::asset::asset_type_for_extension

use khora_core::asset::AssetUUID;

use crate::asset::decoders::decode_material;

/// `true` if [`extract_dependencies`] can extract references from this asset
/// type, i.e. the type has a real extractor rather than the empty fallback.
///
/// The index builder uses this to read file contents **only** for types it can
/// actually parse — textures, audio, meshes, etc. are never read, so adding
/// dependency extraction costs nothing for the assets that have no outbound
/// references. Keep this in sync with the populated arms of
/// [`extract_dependencies`].
pub fn type_has_dependency_extractor(asset_type: &str) -> bool {
    matches!(asset_type, "material")
}

/// Extracts the direct asset dependencies encoded in `bytes`, dispatching on
/// the canonical `asset_type` name.
///
/// Returns a deduplicated, sorted list so the index builder stays
/// byte-deterministic. Unknown or not-yet-handled types return an empty list —
/// this is the extensible seam (see the module docs). A decode failure on a
/// handled type also yields an empty list (logged at `warn`): a single corrupt
/// asset must never abort the whole index build.
pub fn extract_dependencies(asset_type: &str, bytes: &[u8]) -> Vec<AssetUUID> {
    let mut deps = match asset_type {
        "material" => material_dependencies(bytes),
        // Scenes reference entities/prefabs/meshes, but the scene schema is
        // still evolving; their references are not extracted yet.
        "scene" => Vec::new(),
        // Prefabs reference component assets; not extracted yet.
        "prefab" => Vec::new(),
        // Meshes (glTF/glb/obj) embed or reference textures and materials;
        // format-specific parsing is not done yet.
        "mesh" => Vec::new(),
        // Leaf assets (textures, audio, shaders, fonts, scripts, blobs) have no
        // outbound asset references.
        _ => Vec::new(),
    };
    deps.sort();
    deps.dedup();
    deps
}

/// Collects the texture UUIDs referenced by a `.kmat` material.
///
/// Decodes the bytes via [`decode_material`] (no runtime asset service needed)
/// and gathers every `Some(uuid)` the [`Material`] trait's texture accessors
/// expose. A material that references the same texture in several slots
/// contributes that UUID once (the caller dedups). On decode failure the
/// material is skipped with a `warn` and contributes nothing.
///
/// [`Material`]: khora_core::asset::Material
fn material_dependencies(bytes: &[u8]) -> Vec<AssetUUID> {
    let Some(material) = decode_material(bytes) else {
        log::warn!("asset index: skipping unreadable material while extracting dependencies");
        return Vec::new();
    };

    [
        material.base_color_texture(),
        material.metallic_roughness_texture(),
        material.normal_map(),
        material.emissive_texture(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetUUID, StandardMaterial};
    use khora_data::ecs::material_to_json;

    /// Renders a `StandardMaterial` as the `.kmat` RON bytes the decoder
    /// consumes (the `{ type_name, material }` value tree, RON-encoded).
    fn material_to_kmat(material: &StandardMaterial) -> Vec<u8> {
        let json = material_to_json(material).expect("material should serialize to JSON");
        ron::ser::to_string(&json)
            .expect("material JSON should encode to RON")
            .into_bytes()
    }

    #[test]
    fn material_with_two_textures_yields_both_sorted_deduped() {
        let base = AssetUUID::new_v5("textures/base.png");
        let normal = AssetUUID::new_v5("textures/normal.png");
        let material = StandardMaterial {
            base_color_texture: Some(base),
            // Reuse the base-color texture in a second slot to exercise dedup.
            metallic_roughness_texture: Some(base),
            normal_map: Some(normal),
            ..Default::default()
        };

        let deps = extract_dependencies("material", &material_to_kmat(&material));

        let mut expected = vec![base, normal];
        expected.sort();
        assert_eq!(deps, expected, "deps must contain both textures, deduped");
        assert_eq!(deps.len(), 2, "the reused texture appears once");
        assert!(deps.windows(2).all(|w| w[0] <= w[1]), "deps must be sorted");
    }

    #[test]
    fn material_without_textures_yields_empty() {
        let material = StandardMaterial::default();
        let deps = extract_dependencies("material", &material_to_kmat(&material));
        assert!(deps.is_empty(), "an untextured material has no dependencies");
    }

    #[test]
    fn corrupt_material_yields_empty_without_panic() {
        let deps = extract_dependencies("material", b"this is not a valid kmat");
        assert!(deps.is_empty(), "a corrupt material contributes no deps");
    }

    #[test]
    fn unhandled_type_yields_empty() {
        // Textures, scenes, etc. are not parsed for outbound references.
        assert!(extract_dependencies("texture", b"\x89PNG\r\n").is_empty());
        assert!(extract_dependencies("scene", b"(entities: [])").is_empty());
        assert!(extract_dependencies("mesh", b"glTF binary").is_empty());
    }
}
