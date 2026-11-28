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

//! Every read of a project's files going through the guard.

use super::*;

/// **Every read goes through the guard.**
///
/// A glTF buffer or image, a script, a shader overlay — each refused when it
/// leaves its root, by `..` or through a link inside it, or is over its cap.
mod every_read_goes_through_the_guard {
    use std::fs;
    use std::sync::Arc;

    use khora_core::problems::Problems;

    use super::{layout, link_dir, reported_to, security, sized, MIB};
    use crate::asset::decoders::mesh::{FileSystemResolver, GltfResourceResolver};
    use crate::script::compile::DiskLoader;
    use crate::systems::shader_hot_reload::overlay_source;
    use khora_core::asset::AssetUUID;
    use khora_core::math::Vec3;
    use khora_core::renderer::api::gpu_scene::Mesh;
    use khora_script::modules::SourceLoader;

    /// The positions of a triangle, three `f32` VEC3: the bytes of its buffer.
    fn triangle_buffer() -> Vec<u8> {
        [0.0f32, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 8.0, 7.0]
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect()
    }

    /// Returns a `.gltf` showing one triangle whose positions are the external buffer `uri`.
    fn triangle_gltf(uri: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [ { "uri": uri, "byteLength": 36 } ],
            "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 36 } ],
            "accessors": [ {
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [8.0, 8.0, 7.0],
            } ],
            "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 } } ] } ],
        }))
        .expect("the glTF serialises")
    }

    /// Returns the positions of the glTF `models/a.gltf`, loaded out of the unsigned pack `out` as a packed game does.
    fn packed_positions(out: &crate::asset::PackOutput) -> anyhow::Result<Vec<Vec3>> {
        let index = fs::read(&out.index_bin).expect("index.bin reads");
        let loader = crate::asset::PackLoader::new(fs::File::open(&out.data_pack)?)?;
        let mut service = crate::asset::AssetService::new(
            &index,
            Box::new(loader),
            Arc::new(khora_telemetry::MetricsRegistry::new()),
            None,
        )?;
        service.register_packed_decoders();
        let mesh = service.load::<Mesh>(&AssetUUID::new_v5("models/a.gltf"))?;
        Ok(mesh.positions.clone())
    }

    #[test]
    fn a_gltf_buffer_or_image_leaving_the_assets_root_is_refused() {
        let at = layout();
        fs::create_dir_all(at.root.join("models")).expect("the folder is made");
        fs::write(at.root.join("models").join("mesh.bin"), b"buffer").expect("writes");
        fs::write(at.elsewhere.join("secret.bin"), b"secret").expect("writes");
        let resolver = FileSystemResolver::new(&at.root);

        assert_eq!(
            resolver
                .resolve_buffer("models/mesh.bin")
                .expect("a buffer under the root reads"),
            b"buffer"
        );
        assert!(resolver.resolve_buffer("../elsewhere/secret.bin").is_err());
        assert!(resolver.resolve_image("../elsewhere/secret.png").is_err());
        assert!(resolver
            .resolve_buffer(&at.elsewhere.join("secret.bin").display().to_string())
            .is_err());

        if link_dir(&at.elsewhere, &at.root.join("models").join("linked")) {
            assert!(
                resolver.resolve_buffer("models/linked/secret.bin").is_err(),
                "a link inside the root resolving outside, past the lexical URI check"
            );
            assert!(resolver.resolve_image("models/linked/secret.png").is_err());
        }
    }

    /// **A pack carries no external file a link takes out of the assets root**:
    /// the glTF naming it is packed, the bytes behind the link are not.
    #[test]
    fn a_pack_carries_no_external_file_behind_a_link_leaving_the_root() {
        const SECRET: &[u8] = b"bytes of a file beside the project";
        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        fs::write(at.elsewhere.join("secret.bin"), SECRET).expect("writes");
        if !link_dir(&at.elsewhere, &models.join("linked")) {
            return;
        }
        let gltf = br#"{"asset":{"version":"2.0"},
            "buffers":[{"uri":"linked/secret.bin","byteLength":34}]}"#;
        fs::write(models.join("a.gltf"), gltf).expect("writes");
        let dest = tempfile::TempDir::new().expect("a temp dir");

        let out = crate::asset::PackBuilder::new(&at.root, dest.path())
            .build()
            .expect("the project packs");

        let pack = fs::read(&out.data_pack).expect("data.pack reads");
        assert!(
            !pack.windows(SECRET.len()).any(|bytes| bytes == SECRET),
            "the file behind the link is packed"
        );
        let index = crate::asset::AssetIndex::new(&fs::read(&out.index_bin).expect("reads"))
            .expect("the index reads");
        assert_eq!(index.external_file("models/linked/secret.bin"), None);
    }

    /// **A packed glTF whose buffer was left out fails to load, naming the buffer**, never panics.
    #[test]
    fn a_packed_gltf_whose_buffer_was_left_out_fails_to_load_naming_it() {
        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        fs::write(at.elsewhere.join("secret.bin"), triangle_buffer()).expect("writes");
        if !link_dir(&at.elsewhere, &models.join("linked")) {
            return;
        }
        fs::write(models.join("a.gltf"), triangle_gltf("linked/secret.bin")).expect("writes");
        let dest = tempfile::TempDir::new().expect("a temp dir");

        let out = crate::asset::PackBuilder::new(&at.root, dest.path())
            .build()
            .expect("the project packs");

        let error = packed_positions(&out).expect_err("a glTF without its buffer loads");
        assert!(
            format!("{error:#}").contains("models/linked/secret.bin"),
            "the error names the missing buffer: {error:#}"
        );
    }

    /// **A link staying inside the assets root is followed**: the glTF's buffer
    /// behind it is packed under the path the glTF names, and read from it.
    #[test]
    fn a_pack_carries_an_external_file_through_a_link_staying_in_the_root() {
        let at = layout();
        let (models, shared) = (at.root.join("models"), at.root.join("shared"));
        fs::create_dir_all(&models).expect("the folder is made");
        fs::create_dir_all(&shared).expect("the folder is made");
        fs::write(shared.join("tri.bin"), triangle_buffer()).expect("writes");
        if !link_dir(&shared, &models.join("linked")) {
            return;
        }
        fs::write(models.join("a.gltf"), triangle_gltf("linked/tri.bin")).expect("writes");
        let dest = tempfile::TempDir::new().expect("a temp dir");

        let out = crate::asset::PackBuilder::new(&at.root, dest.path())
            .build()
            .expect("the project packs");

        let index = crate::asset::AssetIndex::new(&fs::read(&out.index_bin).expect("reads"))
            .expect("the index reads");
        assert!(index.external_file("models/linked/tri.bin").is_some());
        let positions = packed_positions(&out).unwrap_or_else(|e| panic!("it loads: {e:#}"));
        assert_eq!(positions.len(), 3);
    }

    /// **A glTF naming its buffer in another case reads it from the pack**, as
    /// it reads it from the project on a file system that ignores case.
    #[cfg(windows)]
    #[test]
    fn a_gltf_naming_its_buffer_in_another_case_reads_it_from_the_pack() {
        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        fs::write(models.join("tri.bin"), triangle_buffer()).expect("writes");
        fs::write(models.join("a.gltf"), triangle_gltf("TRI.BIN")).expect("writes");
        let dest = tempfile::TempDir::new().expect("a temp dir");

        let out = crate::asset::PackBuilder::new(&at.root, dest.path())
            .build()
            .expect("the project packs");

        let positions = packed_positions(&out).unwrap_or_else(|e| panic!("it loads: {e:#}"));
        assert_eq!(positions.len(), 3);
    }

    /// **An external file over its container's cap is left out of the pack**,
    /// as one a link takes out of the root is: the pack still builds.
    #[test]
    fn an_external_file_over_its_containers_cap_is_left_out_and_the_pack_builds() {
        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        sized(&models.join("huge.bin"), 1024 * MIB + 1);
        fs::write(models.join("a.gltf"), triangle_gltf("huge.bin")).expect("writes");
        let dest = tempfile::TempDir::new().expect("a temp dir");

        let out = crate::asset::PackBuilder::new(&at.root, dest.path())
            .build()
            .unwrap_or_else(|e| panic!("a refused external file fails the build: {e:#}"));

        let index = crate::asset::AssetIndex::new(&fs::read(&out.index_bin).expect("reads"))
            .expect("the index reads");
        assert_eq!(index.external_file("models/huge.bin"), None);
    }

    /// **A pack build reports its refusals to the refusals it is given**: a
    /// link leaving the root and an external file over its cap both reach them.
    #[test]
    fn a_pack_builds_refusals_reach_the_given_refusals() {
        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        sized(&models.join("huge.bin"), 1024 * MIB + 1);
        let linked = link_dir(&at.elsewhere, &models.join("linked"));
        fs::write(models.join("a.gltf"), triangle_gltf("huge.bin")).expect("writes");
        fs::write(models.join("b.gltf"), triangle_gltf("linked/secret.png")).expect("writes");
        let problems = Arc::new(Problems::default());
        let dest = tempfile::TempDir::new().expect("a temp dir");

        crate::asset::PackBuilder::new(&at.root, dest.path())
            .with_refusals(reported_to(&problems))
            .build()
            .unwrap_or_else(|e| panic!("the project packs: {e:#}"));

        let reported = security(&problems);
        let names = |file: &str| {
            reported
                .iter()
                .any(|p| p.module.as_deref().is_some_and(|path| path.contains(file)))
        };
        assert!(names("huge.bin"), "the file over its cap: {reported:?}");
        if linked {
            assert!(
                names("secret.png"),
                "the link leaving the root: {reported:?}"
            );
        }
    }

    #[test]
    fn a_script_leaving_its_root_or_over_its_cap_is_refused() {
        let at = layout();
        let scripts = at.root.join("scripts");
        fs::create_dir_all(&scripts).expect("the folder is made");
        fs::write(scripts.join("guard.erg"), "behavior Guard {}\n").expect("writes");
        fs::write(at.elsewhere.join("secret.erg"), "behavior Secret {}\n").expect("writes");
        sized(&scripts.join("long.erg"), 64 * MIB + 1);
        let loader = DiskLoader::new(&scripts);

        assert!(
            loader.load("guard.erg").is_some(),
            "a script under its root"
        );
        assert!(loader.load("../../elsewhere/secret.erg").is_none());
        assert!(loader
            .load(&at.elsewhere.join("secret.erg").display().to_string())
            .is_none());
        assert!(loader.load("long.erg").is_none(), "a script over its cap");

        if link_dir(&at.elsewhere, &scripts.join("linked")) {
            assert!(
                loader.load("linked/secret.erg").is_none(),
                "a link inside the root resolving outside"
            );
        }
    }

    #[test]
    fn a_shader_overlay_leaving_its_root_or_over_its_cap_is_refused() {
        let at = layout();
        let lib = at.root.join("shaders").join("lib").join("std");
        fs::create_dir_all(&lib).expect("the folder is made");
        fs::write(lib.join("camera.wgsl"), "// camera\n").expect("writes");
        fs::write(at.elsewhere.join("evil.wgsl"), "// evil\n").expect("writes");
        sized(&lib.join("huge.wgsl"), 64 * MIB + 1);
        let problems = Arc::new(Problems::default());
        let refusals = reported_to(&problems);

        assert_eq!(
            overlay_source(&at.root, "shaders/lib/std/camera.wgsl", &refusals).as_deref(),
            Some("// camera\n")
        );
        assert_eq!(
            overlay_source(&at.root, "../elsewhere/evil.wgsl", &refusals),
            None
        );
        assert_eq!(
            overlay_source(&at.root, "shaders/lib/std/huge.wgsl", &refusals),
            None,
            "an overlay over its cap"
        );
        assert!(!security(&problems).is_empty(), "the refusals are reported");

        if link_dir(&at.elsewhere, &at.root.join("shaders").join("linked")) {
            assert_eq!(
                overlay_source(&at.root, "shaders/linked/evil.wgsl", &refusals),
                None,
                "a link inside the root resolving outside"
            );
        }
    }
}
