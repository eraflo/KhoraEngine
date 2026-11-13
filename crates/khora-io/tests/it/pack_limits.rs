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

//! **What a pack holds is read inside the pack, and checked before anything
//! is allocated for it.**
//! - An entry whose index declares more uncompressed bytes than its manifest
//!   records, or than its type may hold, is refused `TooLarge` before it is
//!   decompressed, and reported.
//! - A packed glTF reads its buffers out of the pack, never from a file
//!   beside it.
//!
//! Every folder here is a temporary one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::asset::{AssetMetadata, AssetSource, AssetUUID, CompressionKind};
use khora_core::math::Vec3;
use khora_core::problems::{Problem, ProblemSource, Problems};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::shader::CpuShaderSource;
use khora_io::asset::signing::{PackSigner, SignedManifest};
use khora_io::asset::{AssetService, PackBuilder, PackLoader, PackManifest, PackOutput};
use khora_io::trust::{Refusals, SecurityReason, TrustLayer};
use khora_telemetry::MetricsRegistry;

const MIB: u64 = 1 << 20;

/// A packed project: its files written under `<dir>/project/assets/`, packed
/// with `compression` into `<dir>/exe/` — signed by the key kept in
/// `<dir>/signing/` when `signed`, with no manifest otherwise.
fn pack(
    files: &[(&str, &[u8])],
    compression: CompressionKind,
    signed: bool,
) -> (tempfile::TempDir, PackOutput) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let assets = dir.path().join("project").join("assets");
    for (rel, bytes) in files {
        let path = assets.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
        std::fs::write(path, bytes).expect("the file is written");
    }
    let exe = dir.path().join("exe");
    let builder = PackBuilder::new(&assets, &exe).with_compression(compression);
    let out = if signed {
        let signer = PackSigner::load_or_create(&dir.path().join("signing"))
            .expect("a signing key in the temporary folder");
        builder.with_signer(&signer).build()
    } else {
        builder.build()
    }
    .expect("the project packs");
    (dir, out)
}

/// The manifest of the signed pack `out`, verified with the key it was
/// signed by (kept in `<dir>/signing/`, beside `<dir>/exe/`) over the
/// `index.bin` the build wrote.
fn signed_manifest(out: &PackOutput) -> PackManifest {
    let dir = (out.index_bin.parent())
        .and_then(Path::parent)
        .expect("the pack's folder");
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

/// The index of `out`, with the entry of `rel` changed by `forge`.
fn forged_index(out: &PackOutput, rel: &str, forge: impl FnOnce(&mut AssetSource)) -> Vec<u8> {
    let config = bincode::config::standard();
    let bytes = std::fs::read(&out.index_bin).expect("index.bin reads");
    let (mut entries, _): (Vec<AssetMetadata>, _) =
        bincode::serde::decode_from_slice(&bytes, config).expect("index.bin decodes");
    let uuid = AssetUUID::new_v5(rel);
    let entry = entries
        .iter_mut()
        .find(|entry| entry.uuid == uuid)
        .unwrap_or_else(|| panic!("{rel} is packed"));
    forge(
        entry
            .variants
            .get_mut("default")
            .expect("a packed entry has a default source"),
    );
    bincode::serde::encode_to_vec(&entries, config).expect("the index encodes")
}

/// A service over the pack `out` with the forged `index`, the inventory
/// decoders registered, refusing to `problems` — with the pack's manifest
/// when `manifest`.
fn service_over(
    out: &PackOutput,
    index: &[u8],
    manifest: bool,
    problems: &Arc<Problems>,
) -> (AssetService, Arc<Refusals>) {
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).expect("data.pack opens"))
        .expect("a valid pack header");
    let manifest = manifest.then(|| signed_manifest(out));
    let mut service = AssetService::new(
        index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        manifest,
    )
    .expect("a service over the pack");
    service.register_inventory_decoders();
    let refusals = Arc::new(Refusals::new(Some(Arc::clone(problems)), None));
    service.set_refusals(Arc::clone(&refusals));
    (service, refusals)
}

/// The `Security` problem naming the file whose path ends with `name`.
fn reported(problems: &Problems, name: &str) -> Option<Problem> {
    problems
        .of_source(&ProblemSource::Security)
        .into_iter()
        .find(|problem| {
            problem
                .module
                .as_deref()
                .is_some_and(|module| module.ends_with(name))
        })
}

/// A shader LZ4 compresses well, and one that stays as it is.
fn shaders() -> (Vec<u8>, Vec<u8>) {
    (
        "// a note that repeats\n".repeat(100).into_bytes(),
        b"// fine\n".to_vec(),
    )
}

/// Sets a packed source's uncompressed size to `size`.
fn claiming(size: u64) -> impl FnOnce(&mut AssetSource) {
    move |source| match source {
        AssetSource::Packed {
            uncompressed_size,
            compression,
            ..
        } => {
            assert_eq!(
                *compression,
                CompressionKind::Lz4,
                "the forged entry is compressed"
            );
            *uncompressed_size = size;
        }
        other => panic!("a packed source, not {other:?}"),
    }
}

/// Checks the refusal of `notes.wgsl` reported to `problems` and, when it is
/// quarantined, held by `refusals`: `TooLarge` of `size` over `cap`, at the
/// pack layer.
fn assert_refused_at_the_pack(problems: &Problems, refusals: &Refusals, size: u64, cap: u64) {
    let problem = reported(problems, "notes.wgsl").unwrap_or_else(|| {
        panic!(
            "the forged entry is refused and reported: {:?}",
            problems.of_source(&ProblemSource::Security)
        )
    });
    assert!(
        problem.message.contains(&size.to_string()) && problem.message.contains(&cap.to_string()),
        "refused as {size} bytes over {cap}: {}",
        problem.message
    );
    if let Some(refusal) = refusals
        .quarantined()
        .into_iter()
        .find(|refusal| refusal.path.ends_with("notes.wgsl"))
    {
        assert_eq!(refusal.reason, SecurityReason::TooLarge { size, cap });
        assert_eq!(refusal.layer, TrustLayer::Pack);
    }
}

/// **A pack entry with a bomb ratio is refused before allocating**, against
/// its manifest: an LZ4 entry whose index claims a thousand bytes more than
/// the manifest records for it is refused `TooLarge` over the manifest's
/// size, at the pack layer, before it is decompressed; an honest entry of
/// the same pack loads.
#[test]
fn a_pack_entry_with_a_bomb_ratio_is_refused_before_allocating() {
    let (notes, fine) = shaders();
    let (_dir, out) = pack(
        &[("shaders/notes.wgsl", &notes), ("shaders/fine.wgsl", &fine)],
        CompressionKind::Lz4,
        true,
    );
    let actual = notes.len() as u64;
    let claimed = actual + 1000;
    let index = forged_index(&out, "shaders/notes.wgsl", claiming(claimed));
    let problems = Arc::new(Problems::default());
    let (mut service, refusals) = service_over(&out, &index, true, &problems);

    assert!(service
        .load::<CpuShaderSource>(&AssetUUID::new_v5("shaders/notes.wgsl"))
        .is_err());
    assert_refused_at_the_pack(&problems, &refusals, claimed, actual);

    service
        .load::<CpuShaderSource>(&AssetUUID::new_v5("shaders/fine.wgsl"))
        .expect("an honest entry of the pack loads");
}

/// **A pack entry with a bomb ratio is refused before allocating, without a
/// manifest**: an LZ4 entry whose index claims one byte more than a shader
/// may hold (64 MiB at the project's trust) is refused `TooLarge` over that
/// cap, at the pack layer, before it is decompressed.
#[test]
fn a_pack_entry_with_a_bomb_ratio_is_refused_without_a_manifest() {
    let (notes, fine) = shaders();
    let (_dir, out) = pack(
        &[("shaders/notes.wgsl", &notes), ("shaders/fine.wgsl", &fine)],
        CompressionKind::Lz4,
        false,
    );
    let claimed = 64 * MIB + 1;
    let index = forged_index(&out, "shaders/notes.wgsl", claiming(claimed));
    let problems = Arc::new(Problems::default());
    let (mut service, refusals) = service_over(&out, &index, false, &problems);

    assert!(service
        .load::<CpuShaderSource>(&AssetUUID::new_v5("shaders/notes.wgsl"))
        .is_err());
    assert_refused_at_the_pack(&problems, &refusals, claimed, 64 * MIB);

    service
        .load::<CpuShaderSource>(&AssetUUID::new_v5("shaders/fine.wgsl"))
        .expect("an honest entry of the pack loads");
}

// ─── A packed glTF ──────────────────────────────────────────────────────────

/// The triangle packed with the glTF.
const PACKED: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// The triangle of the decoy buffer left beside the pack.
const DECOY: [[f32; 3]; 3] = [[7.0, 7.0, 7.0], [8.0, 7.0, 7.0], [7.0, 8.0, 7.0]];

/// A buffer holding `corners` as three `f32` VEC3.
fn buffer(corners: &[[f32; 3]; 3]) -> Vec<u8> {
    corners
        .iter()
        .flatten()
        .flat_map(|c| c.to_le_bytes())
        .collect()
}

/// A `.gltf` showing one triangle whose positions are the external buffer
/// `tri.bin`, beside it.
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

fn positions(corners: &[[f32; 3]; 3]) -> Vec<Vec3> {
    corners
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect()
}

/// **A packed glTF reads its buffers inside the pack**: a pack holding
/// `models/tri.gltf` and its `models/tri.bin`, served with the packed
/// decoders, decodes the triangle the pack holds — whole and as its
/// primitive — though a `models/tri.bin` holding another triangle sits
/// beside the pack, where a loose read would find it.
#[test]
fn a_packed_gltf_reads_its_buffers_inside_the_pack() {
    let (_dir, out) = pack(
        &[
            ("models/tri.gltf", &triangle_gltf()),
            ("models/tri.bin", &buffer(&PACKED)),
        ],
        CompressionKind::None,
        false,
    );
    let exe: PathBuf = out
        .data_pack
        .parent()
        .expect("the pack's folder")
        .to_path_buf();
    let decoy = exe.join("models").join("tri.bin");
    std::fs::create_dir_all(decoy.parent().expect("a folder")).expect("the decoy's folder");
    std::fs::write(&decoy, buffer(&DECOY)).expect("the decoy is written");

    let index = std::fs::read(&out.index_bin).expect("index.bin reads");
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).expect("data.pack opens"))
        .expect("a valid pack header");
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .expect("a service over the pack");
    service.register_packed_decoders();

    let whole = AssetUUID::new_v5("models/tri.gltf");
    let mesh = service
        .load::<Mesh>(&whole)
        .unwrap_or_else(|e| panic!("the packed glTF decodes: {e:#}"));
    assert_eq!(
        mesh.positions,
        positions(&PACKED),
        "the buffer read is the pack's, not the one beside it"
    );

    let part = service
        .load::<Mesh>(&AssetUUID::sub_asset(whole, "mesh0/prim0"))
        .unwrap_or_else(|e| panic!("the packed glTF's primitive extracts: {e:#}"));
    assert_eq!(part.positions, positions(&PACKED));
}

/// A `.gltf` showing `triangles` triangles whose positions are the external
/// buffer `many.bin`, beside it, and that buffer — every triangle the same,
/// so LZ4 compresses it.
fn many_triangles(triangles: usize) -> (Vec<u8>, Vec<u8>) {
    let bin: Vec<u8> = (0..triangles).flat_map(|_| buffer(&PACKED)).collect();
    let gltf = serde_json::to_vec(&serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [ { "uri": "many.bin", "byteLength": bin.len() } ],
        "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": bin.len() } ],
        "accessors": [ {
            "bufferView": 0, "componentType": 5126, "count": triangles * 3, "type": "VEC3",
            "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0],
        } ],
        "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 } } ] } ],
        "nodes": [ { "mesh": 0 } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0,
    }))
    .expect("the glTF serialises");
    (gltf, bin)
}

/// **A packed glTF's buffer with a bomb ratio is refused before
/// allocating**: the `.bin` a packed glTF reads, an LZ4 entry whose index
/// claims a thousand bytes more than the manifest records for it, is
/// refused `TooLarge` over the manifest's size, at the pack layer, and
/// reported — as the same entry is when it is loaded on its own.
#[test]
fn a_packed_gltf_buffer_with_a_bomb_ratio_is_refused_before_allocating() {
    let (gltf, bin) = many_triangles(100);
    let (_dir, out) = pack(
        &[("models/many.gltf", &gltf), ("models/many.bin", &bin)],
        CompressionKind::Lz4,
        true,
    );
    let actual = bin.len() as u64;
    let claimed = actual + 1000;
    let index = forged_index(&out, "models/many.bin", claiming(claimed));
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).expect("data.pack opens"))
        .expect("a valid pack header");
    let manifest = signed_manifest(&out);
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        Some(manifest),
    )
    .expect("a service over the pack");
    let problems = Arc::new(Problems::default());
    service.set_refusals(Arc::new(Refusals::new(Some(Arc::clone(&problems)), None)));
    service.register_packed_decoders();

    assert!(service
        .load::<Mesh>(&AssetUUID::new_v5("models/many.gltf"))
        .is_err());
    let problem = reported(&problems, "many.bin").unwrap_or_else(|| {
        panic!(
            "the forged buffer is refused and reported: {:?}",
            problems.of_source(&ProblemSource::Security)
        )
    });
    assert!(
        problem.message.contains(&claimed.to_string())
            && problem.message.contains(&actual.to_string()),
        "refused as {claimed} bytes over {actual}: {}",
        problem.message
    );
}

/// **A packed scene has a text file's cap**: a 20 MiB `.kscene` — under the
/// 64 MiB a project's text file may hold, as it reads loose — is read raw
/// out of a pack with no manifest, not refused at the strictest 16 MiB for
/// having no decoder.
#[test]
fn a_packed_scene_has_a_text_file_cap() {
    let len = 20 * MIB as usize;
    let scene = vec![b' '; len];
    let (_dir, out) = pack(
        &[("scenes/big.kscene", &scene)],
        CompressionKind::None,
        false,
    );
    let index = std::fs::read(&out.index_bin).expect("index.bin reads");
    let problems = Arc::new(Problems::default());
    let (mut service, _refusals) = service_over(&out, &index, false, &problems);

    let bytes = service
        .load_raw(&AssetUUID::new_v5("scenes/big.kscene"))
        .unwrap_or_else(|e| panic!("a 20 MiB scene reads out of the pack: {e:#}"));
    assert_eq!(bytes.len(), len);
}
