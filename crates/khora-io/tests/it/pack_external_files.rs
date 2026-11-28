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

//! **A container's external files ride in the pack's own table, never as assets.**
//! - A packed glTF reads its buffers from the external-files table, by path.
//! - An image the index lists is packed once, as an asset, and read from its entry.
//! - The packed index holds only assets: no `.bin`, no README.
//! - An external file's bytes are sealed: changed after the build, they are refused.

use std::path::Path;
use std::sync::Arc;

use khora_core::asset::{asset_key, AssetSource, AssetUUID, CompressionKind};
use khora_core::math::Vec3;
use khora_core::problems::{ProblemSource, Problems};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_io::asset::signing::{PackSigner, SignedManifest};
use khora_io::asset::{
    kind_of_path, AssetIndex, AssetService, PackBuilder, PackLoader, PackManifest, PackOutput,
    PACK_HEADER_SIZE,
};
use khora_io::trust::Refusals;
use khora_telemetry::MetricsRegistry;

/// The triangle the packed buffer holds.
const CORNERS: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [8.0, 0.0, 0.0], [0.0, 8.0, 7.0]];

/// Returns the buffer holding [`CORNERS`] as three `f32` VEC3.
fn buffer() -> Vec<u8> {
    CORNERS
        .iter()
        .flatten()
        .flat_map(|c| c.to_le_bytes())
        .collect()
}

/// Returns a `.gltf` showing one triangle whose positions are its external buffer `tri.bin`.
fn triangle_gltf() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [ { "uri": "tri.bin", "byteLength": 36 } ],
        "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 36 } ],
        "accessors": [ {
            "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
            "min": [0.0, 0.0, 0.0], "max": [8.0, 8.0, 7.0],
        } ],
        "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 } } ] } ],
        "nodes": [ { "mesh": 0 } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0,
    }))
    .expect("the glTF serialises")
}

/// Returns a PNG of `width` × `height` transparent pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("the png encodes");
    png
}

/// Returns a `.gltf` holding one texture, its image the file `uri` names.
fn gltf_naming_image(uri: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "asset": { "version": "2.0" },
        "images": [ { "uri": uri } ],
        "textures": [ { "source": 0 } ],
    }))
    .expect("the glTF serialises")
}

/// Packs `files` under `<dir>/project/assets` into `<dir>/exe/`, signed by the key kept in `<dir>/signing/`.
fn pack(dir: &Path, files: &[(&str, Vec<u8>)]) -> PackOutput {
    let assets = dir.join("project").join("assets");
    for (rel, bytes) in files {
        let path = assets.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
        std::fs::write(path, bytes).expect("the file is written");
    }
    let signer = PackSigner::load_or_create(&dir.join("signing"))
        .expect("a signing key in the temporary folder");
    PackBuilder::new(&assets, &dir.join("exe"))
        .with_compression(CompressionKind::None)
        .with_signer(&signer)
        .build()
        .expect("the project packs")
}

/// Returns a project of a glTF, its buffer and a README, packed raw into
/// `<dir>/exe/` and signed by the key kept in `<dir>/signing/`.
fn packed_project() -> (tempfile::TempDir, PackOutput) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let assets = dir.path().join("project").join("assets");
    for (rel, bytes) in [
        ("models/tri.gltf", triangle_gltf()),
        ("models/tri.bin", buffer()),
        ("README.md", b"# Assets".to_vec()),
    ] {
        let path = assets.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
        std::fs::write(path, bytes).expect("the file is written");
    }
    let signer = PackSigner::load_or_create(&dir.path().join("signing"))
        .expect("a signing key in the temporary folder");
    let out = PackBuilder::new(&assets, &dir.path().join("exe"))
        .with_compression(CompressionKind::None)
        .with_signer(&signer)
        .build()
        .expect("the project packs");
    (dir, out)
}

/// Returns the manifest of `out`, verified with the key it was signed by.
fn verified_manifest(dir: &Path, out: &PackOutput) -> PackManifest {
    let key = PackSigner::load_or_create(&dir.join("signing"))
        .expect("the key the pack was signed by")
        .public_key();
    let path = out.manifest_bin.as_ref().expect("a manifest is written");
    SignedManifest::decode(&std::fs::read(path).expect("manifest.bin reads"))
        .expect("manifest.bin is a signed manifest")
        .verify(
            &key,
            &std::fs::read(&out.index_bin).expect("index.bin reads"),
        )
        .expect("the manifest verifies")
}

/// Returns the index `out` wrote.
fn packed_index(out: &PackOutput) -> AssetIndex {
    AssetIndex::new(&std::fs::read(&out.index_bin).expect("index.bin reads"))
        .expect("the packed index reads")
}

/// Returns a service over the pack `out`, its packed decoders registered, refusing to `problems`.
fn service_over(
    out: &PackOutput,
    manifest: PackManifest,
    problems: &Arc<Problems>,
) -> AssetService {
    let index = std::fs::read(&out.index_bin).expect("index.bin reads");
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).expect("data.pack opens"))
        .expect("a valid pack header");
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        Some(manifest),
    )
    .expect("a service over the pack");
    service.set_refusals(Arc::new(Refusals::new(Some(Arc::clone(problems)), None)));
    service.register_packed_decoders();
    service
}

/// **A packed glTF reads its buffers from the external-files table**: the
/// buffer is in the table, by its path, and the glTF decodes from it.
#[test]
fn a_packed_gltf_reads_its_buffers_from_the_external_files_table() {
    let (dir, out) = packed_project();
    let index = packed_index(&out);
    let packed = index
        .external_file("models/tri.bin")
        .expect("the buffer is in the external-files table");
    assert_eq!(packed.path, "models/tri.bin");
    assert_eq!(packed.uncompressed_size, buffer().len() as u64);
    assert!(
        index.external_file("models/tri.gltf").is_none(),
        "an asset is no external file"
    );

    let problems = Arc::new(Problems::default());
    let mut service = service_over(&out, verified_manifest(dir.path(), &out), &problems);
    let mesh = service
        .load::<Mesh>(&AssetUUID::new_v5("models/tri.gltf"))
        .unwrap_or_else(|e| panic!("the packed glTF decodes: {e:#}"));
    let expected: Vec<Vec3> = CORNERS
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect();
    assert_eq!(mesh.positions, expected);
}

/// **The packed index holds no entry that is not an asset**: each entry of a
/// file of its own is of the kind its extension names; the buffer and the
/// README are none of them.
#[test]
fn the_packed_index_holds_no_entry_that_is_not_an_asset() {
    let (_dir, out) = packed_project();
    let index = packed_index(&out);
    let mut files: Vec<String> = Vec::new();
    for entry in index.iter_all() {
        if matches!(entry.default_source(), Some(AssetSource::Sub { .. })) {
            continue;
        }
        assert_eq!(
            kind_of_path(&entry.source_path),
            Some(entry.kind),
            "{} is an asset of its own kind",
            entry.source_path.display()
        );
        files.push(asset_key(&entry.source_path));
    }
    assert_eq!(files, ["models/tri.gltf"]);
    assert_eq!(index.uuid_at("models/tri.bin"), None);
    assert_eq!(index.uuid_at("README.md"), None);
}

/// **A packed glTF reads an image the index lists from its asset entry**: the
/// texture is packed once, as an asset, and the glTF's part decodes from it.
#[test]
fn a_packed_gltf_reads_an_image_the_index_lists_from_its_asset_entry() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let out = pack(
        dir.path(),
        &[
            (
                "models/skin.gltf",
                gltf_naming_image("../textures/skin.png"),
            ),
            ("textures/skin.png", png(3, 2)),
        ],
    );
    let index = packed_index(&out);
    assert!(index.uuid_at("textures/skin.png").is_some(), "an asset");
    assert_eq!(
        index.external_files().count(),
        0,
        "an image the index lists is no external file"
    );

    let problems = Arc::new(Problems::default());
    let mut service = service_over(&out, verified_manifest(dir.path(), &out), &problems);
    let part = AssetUUID::sub_asset(AssetUUID::new_v5("models/skin.gltf"), "texture0");
    let texture = service
        .load::<CpuTexture>(&part)
        .unwrap_or_else(|e| panic!("the glTF's image is read from the pack: {e:#}"));
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

/// **Tampering with an external file's bytes breaks the seal**: a byte of the
/// packed buffer changed after the build is refused against the manifest,
/// reported, and the glTF reading it does not decode.
#[test]
fn tampering_with_an_external_files_bytes_breaks_the_seal() {
    let (dir, out) = packed_project();
    let manifest = verified_manifest(dir.path(), &out);
    assert!(
        manifest.external_file("models/tri.bin").is_some(),
        "the manifest records the buffer's bytes"
    );
    let at = packed_index(&out)
        .external_file("models/tri.bin")
        .expect("the buffer is packed")
        .offset;
    let mut pack = std::fs::read(&out.data_pack).expect("data.pack reads");
    let byte = usize::try_from(PACK_HEADER_SIZE + at + 4).expect("a small pack");
    pack[byte] ^= 0x40;
    std::fs::write(&out.data_pack, pack).expect("data.pack is rewritten");

    let problems = Arc::new(Problems::default());
    let mut service = service_over(&out, manifest, &problems);
    assert!(
        service
            .load::<Mesh>(&AssetUUID::new_v5("models/tri.gltf"))
            .is_err(),
        "the glTF reading a tampered buffer does not decode"
    );
    let refused = problems
        .of_source(&ProblemSource::Security)
        .into_iter()
        .find(|problem| {
            problem
                .module
                .as_deref()
                .is_some_and(|module| module.ends_with("tri.bin"))
        })
        .unwrap_or_else(|| {
            panic!(
                "the tampered buffer is reported: {:?}",
                problems.of_source(&ProblemSource::Security)
            )
        });
    assert!(
        refused.message.contains("not what the build signed"),
        "refused as tampered: {}",
        refused.message
    );
}
