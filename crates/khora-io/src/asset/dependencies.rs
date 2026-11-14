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

//! The direct dependencies of an asset, read from its bytes while the index is built.
//!
//! See the book: concepts/assets.md, "Dependency tracking".

use khora_core::asset::AssetUUID;

use crate::asset::decoders::decode_material;

/// Whether [`extract_dependencies`] has a real extractor for `asset_type`.
///
/// The index builder reads a file's bytes only for such a type. Keep it in
/// sync with the populated arms of [`extract_dependencies`].
pub fn type_has_dependency_extractor(asset_type: &str) -> bool {
    matches!(asset_type, "material" | "script")
}

/// Returns the direct asset dependencies encoded in `bytes`, by the `asset_type` name.
///
/// Deduplicated and sorted, so the index stays byte-deterministic. A type with no
/// extractor, or bytes that do not decode, give an empty list (the latter logged
/// at `warn`): one corrupt asset never aborts the index build.
pub fn extract_dependencies(asset_type: &str, path: &str, bytes: &[u8]) -> Vec<AssetUUID> {
    let mut deps = match asset_type {
        "material" => material_dependencies(bytes),
        // An `import` names a path, and a path becomes a UUID only against the
        // root it is relative to — which is why this needs the importing file's
        // own location and the other extractors do not.
        "script" => script_dependencies(path, bytes),
        // Scenes, prefabs and meshes reference other assets; not extracted yet.
        "scene" => Vec::new(),
        "prefab" => Vec::new(),
        "mesh" => Vec::new(),
        // Leaf assets (textures, audio, shaders, fonts, blobs) have no outbound
        // asset references.
        _ => Vec::new(),
    };
    deps.sort();
    deps.dedup();
    deps
}

/// Collects the module UUIDs a `.erg` script imports.
///
/// They are the script's dependencies in the asset index. Script hot reload
/// reads imports from the files instead, an `.erg` edited in place being never
/// reindexed. Bytes that are not UTF-8 contribute nothing: the decoder reports them.
fn script_dependencies(path: &str, bytes: &[u8]) -> Vec<AssetUUID> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        log::warn!("asset index: script '{path}' is not valid UTF-8");
        return Vec::new();
    };

    // An `import` is written from the script folder, not from the importing
    // file — `"combat/damage.erg"` means the same thing wherever it is written,
    // which lets a file move without rewriting its importers. An `.erg` outside
    // that folder is no script: it imports nothing, because nothing compiles it.
    if crate::script::layout::module_of(path).is_none() {
        return Vec::new();
    }
    let root = crate::script::layout::SCRIPT_DIR;
    khora_script::modules::imports_of(source)
        .into_iter()
        .map(|import| AssetUUID::new_v5(&format!("{root}/{import}")))
        .collect()
}

/// Collects the texture UUIDs a `.kmat` material references, possibly repeated.
///
/// A material that does not decode contributes nothing, with a `warn`.
fn material_dependencies(bytes: &[u8]) -> Vec<AssetUUID> {
    let Some(material) = decode_material(bytes) else {
        log::warn!("asset index: skipping unreadable material while extracting dependencies");
        return Vec::new();
    };

    material.textures()
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetUUID, StandardMaterial};
    use khora_data::scene::material_to_json;

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

        let deps =
            extract_dependencies("material", "materials/m.kmat", &material_to_kmat(&material));

        let mut expected = vec![base, normal];
        expected.sort();
        assert_eq!(deps, expected, "deps must contain both textures, deduped");
        assert_eq!(deps.len(), 2, "the reused texture appears once");
        assert!(deps.windows(2).all(|w| w[0] <= w[1]), "deps must be sorted");
    }

    #[test]
    fn material_lists_its_occlusion_and_both_specular_textures() {
        let occlusion = AssetUUID::new_v5("textures/ao.png");
        let specular = AssetUUID::new_v5("textures/specular.png");
        let specular_color = AssetUUID::new_v5("textures/specular_color.png");
        let material = StandardMaterial {
            occlusion_map: Some(occlusion),
            specular_texture: Some(specular),
            specular_color_texture: Some(specular_color),
            ..Default::default()
        };

        let deps =
            extract_dependencies("material", "materials/m.kmat", &material_to_kmat(&material));

        let mut expected = vec![occlusion, specular, specular_color];
        expected.sort();
        assert_eq!(deps, expected, "occlusion, specular and specular colour");
    }

    #[test]
    fn material_lists_every_texture_slot_of_a_kmat() {
        let slots: Vec<AssetUUID> = [
            "base", "mr", "normal", "emissive", "ao", "specular", "spec_col",
        ]
        .iter()
        .map(|name| AssetUUID::new_v5(&format!("textures/{name}.png")))
        .collect();
        let material = StandardMaterial {
            base_color_texture: Some(slots[0]),
            metallic_roughness_texture: Some(slots[1]),
            normal_map: Some(slots[2]),
            emissive_texture: Some(slots[3]),
            occlusion_map: Some(slots[4]),
            specular_texture: Some(slots[5]),
            specular_color_texture: Some(slots[6]),
            ..Default::default()
        };

        let deps =
            extract_dependencies("material", "materials/m.kmat", &material_to_kmat(&material));

        let mut expected = slots;
        expected.sort();
        assert_eq!(deps, expected, "every set slot is a dependency");
    }

    #[test]
    fn material_without_textures_yields_empty() {
        let material = StandardMaterial::default();
        let deps =
            extract_dependencies("material", "materials/m.kmat", &material_to_kmat(&material));
        assert!(
            deps.is_empty(),
            "an untextured material has no dependencies"
        );
    }

    #[test]
    fn corrupt_material_yields_empty_without_panic() {
        let deps = extract_dependencies("material", "a/b.dat", b"this is not a valid kmat");
        assert!(deps.is_empty(), "a corrupt material contributes no deps");
    }

    #[test]
    fn unhandled_type_yields_empty() {
        // Textures, scenes, etc. are not parsed for outbound references.
        assert!(extract_dependencies("texture", "a/b.dat", b"\x89PNG\r\n").is_empty());
        assert!(extract_dependencies("scene", "a/b.dat", b"(entities: [])").is_empty());
        assert!(extract_dependencies("mesh", "a/b.dat", b"glTF binary").is_empty());
    }

    mod script_tests {
        use super::super::*;

        #[test]
        fn a_script_is_a_type_with_an_extractor() {
            assert!(type_has_dependency_extractor("script"));
            assert!(!type_has_dependency_extractor("texture"));
        }

        /// **An import is recorded as a dependency on the module it names.**
        #[test]
        fn an_import_becomes_the_uuid_of_the_file_it_names() {
            let source = br#"import "combat/damage.erg";
                             fn void Main() { }"#;
            let deps = extract_dependencies("script", "scripts/ai/guard.erg", source);

            assert_eq!(deps, vec![AssetUUID::new_v5("scripts/combat/damage.erg")]);
        }

        /// An import means the same thing wherever it is written, which is what
        /// lets a file be moved without rewriting the files that import it.
        #[test]
        fn an_import_is_relative_to_the_root_not_to_the_importing_file() {
            let source = br#"import "shared.erg"; fn void Main() { }"#;

            let shallow = extract_dependencies("script", "scripts/a.erg", source);
            let deep = extract_dependencies("script", "scripts/very/deep/b.erg", source);

            assert_eq!(shallow, deep, "the same import names the same module");
            assert_eq!(shallow, vec![AssetUUID::new_v5("scripts/shared.erg")]);
        }

        /// **A file at the top of the assets is no script**: it is never compiled,
        /// so what it imports is no edge for hot-reload to walk.
        #[test]
        fn an_erg_at_the_top_level_has_no_edges() {
            let source = br#"import "helper.erg"; fn void Main() { }"#;
            let deps = extract_dependencies("script", "guard.erg", source);

            assert!(deps.is_empty(), "{deps:?}");
        }

        /// **An `.erg` outside `scripts/` has no edges**, whatever folder or case it is under.
        #[test]
        fn an_erg_outside_the_script_folder_has_no_edges() {
            let source = br#"import "helper.erg"; fn void Main() { }"#;

            for path in [
                "props/x.erg",
                "Scripts/x.erg",
                "scriptsextra/x.erg",
                "props/scripts/x.erg",
            ] {
                let deps = extract_dependencies("script", path, source);
                assert!(deps.is_empty(), "{path}: {deps:?}");
            }
        }

        /// A module's extension is `.erg` in lower case: a file in place spelled
        /// `.ERG` is no module — nothing compiles it — and imports nothing.
        #[test]
        fn a_file_in_place_with_a_capital_extension_has_no_edges() {
            let source = br#"import "combat/damage.erg"; fn void Main() { }"#;
            let deps = extract_dependencies("script", "scripts/ai/Guard.ERG", source);

            assert!(deps.is_empty(), "{deps:?}");
        }

        #[test]
        fn a_script_with_no_imports_has_no_dependencies() {
            let deps = extract_dependencies("script", "scripts/leaf.erg", b"fn void Main() { }");
            assert!(deps.is_empty());
        }

        /// Deduplicated and sorted, like every other extractor — the index
        /// builder's contract is byte-determinism.
        #[test]
        fn dependencies_come_back_sorted_and_deduplicated() {
            let source = br#"import "b.erg";
                             import "a.erg";
                             import "b.erg";
                             fn void Main() { }"#;
            let deps = extract_dependencies("script", "scripts/main.erg", source);

            assert_eq!(deps.len(), 2);
            let mut sorted = deps.clone();
            sorted.sort();
            assert_eq!(deps, sorted);
        }

        /// Reported once, by the decoder — putting the same message in the log
        /// twice for one mistake helps nobody.
        #[test]
        fn bytes_that_are_not_text_contribute_nothing() {
            let deps = extract_dependencies("script", "scripts/broken.erg", &[0xff, 0xfe]);
            assert!(deps.is_empty());
        }
    }
}
