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

use std::fs;
use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_telemetry::MetricsRegistry;
use tempfile::tempdir;

use super::*;
use crate::asset::signing::{PackSigner, SignedManifest};
use crate::asset::{AssetIndex, AssetService, PackLoader, REGISTRY_DIR, REGISTRY_FILE};

fn seed_project(root: &Path) {
    fs::create_dir_all(root.join("textures")).unwrap();
    fs::create_dir_all(root.join("scenes")).unwrap();
    fs::write(root.join("textures").join("a.png"), b"PNG-A").unwrap();
    fs::write(root.join("textures").join("b.png"), b"PNG-B").unwrap();
    fs::write(root.join("scenes").join("default.kscene"), b"SCENE").unwrap();
}

#[test]
fn build_produces_index_and_pack() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_project(proj.path());

    let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();

    assert!(out.index_bin.exists());
    assert!(out.data_pack.exists());
    assert_eq!(out.asset_count, 3);
    assert_eq!(out.pack_bytes(), 5 + 5 + 5); // PNG-A + PNG-B + SCENE
}

#[test]
fn build_is_deterministic() {
    let proj = tempdir().unwrap();
    let dest_a = tempdir().unwrap();
    let dest_b = tempdir().unwrap();
    seed_project(proj.path());

    let _ = PackBuilder::new(proj.path(), dest_a.path())
        .build()
        .unwrap();
    let _ = PackBuilder::new(proj.path(), dest_b.path())
        .build()
        .unwrap();

    let idx_a = std::fs::read(dest_a.path().join("index.bin")).unwrap();
    let idx_b = std::fs::read(dest_b.path().join("index.bin")).unwrap();
    assert_eq!(idx_a, idx_b, "index.bin must be byte-deterministic");

    let pack_a = std::fs::read(dest_a.path().join("data.pack")).unwrap();
    let pack_b = std::fs::read(dest_b.path().join("data.pack")).unwrap();
    assert_eq!(pack_a, pack_b, "data.pack must be byte-deterministic");
}

#[test]
fn pack_round_trips_through_packloader() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_project(proj.path());

    let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();

    // Each asset read back through the index and a PackLoader is the
    // bytes appended: offsets skip the header.
    let index_bytes = std::fs::read(&out.index_bin).unwrap();
    let vfs = AssetIndex::new(&index_bytes).unwrap();
    let pack_file = std::fs::File::open(&out.data_pack).unwrap();
    let pack_loader = PackLoader::new(pack_file).expect("valid pack header");
    assert_eq!(pack_loader.header().asset_count, vfs.asset_count() as u32);
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(&index_bytes, Box::new(pack_loader), metrics, None).unwrap();

    // Verify each asset by raw load_raw, since we don't register any
    // decoders here (binary blobs aren't real PNGs).
    let known = [
        ("scenes/default.kscene", &b"SCENE"[..]),
        ("textures/a.png", &b"PNG-A"[..]),
        ("textures/b.png", &b"PNG-B"[..]),
    ];
    for (rel, expected) in known {
        let uuid = AssetUUID::new_v5(rel);
        assert!(
            vfs.get_metadata(&uuid).is_some(),
            "uuid for {} missing",
            rel
        );
        let bytes = svc.load_raw(&uuid).unwrap();
        assert_eq!(bytes, expected, "round-trip mismatch for {}", rel);
    }
}

#[test]
fn progress_channel_emits_started_and_finished() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_project(proj.path());

    let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
    let _ = PackBuilder::new(proj.path(), dest.path())
        .with_progress(tx)
        .build()
        .unwrap();

    let mut started = 0usize;
    let mut finished = 0usize;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            PackProgress::Started { current, total, .. } => {
                assert_eq!(total, 3);
                assert!(current < total);
                started += 1;
            }
            PackProgress::Finished { asset_count, .. } => {
                assert_eq!(asset_count, 3);
                finished += 1;
            }
            PackProgress::Failed { .. } => panic!("unexpected Failed event"),
        }
    }
    assert_eq!(started, 3);
    assert_eq!(finished, 1);
}

/// **A build that fails says so on its progress channel**, naming the asset
/// it refused, and never says it finished.
#[test]
fn a_build_refusing_an_asset_reports_failed_on_its_progress_channel() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_project(proj.path());
    fs::create_dir_all(proj.path().join("materials")).unwrap();
    // One byte over a trusted project's cap of a text file.
    fs::File::create(proj.path().join("materials").join("big.kmat"))
        .and_then(|file| file.set_len((64 << 20) + 1))
        .unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
    let built = PackBuilder::new(proj.path(), dest.path())
        .with_progress(tx)
        .build();
    assert!(built.is_err(), "a refused asset fails the build");

    let events: Vec<PackProgress> = rx.try_iter().collect();
    assert!(
        !events
            .iter()
            .any(|ev| matches!(ev, PackProgress::Finished { .. })),
        "a failed build never finished: {events:?}"
    );
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            PackProgress::Failed { message } if message.contains("big.kmat")
        )),
        "the failure is on the progress channel, naming the asset: {events:?}"
    );
}

/// **A build lists every external file it left out, and why**: one over
/// its container's cap, one missing; the build still succeeds.
#[test]
fn a_build_lists_every_external_file_it_left_out() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let models = proj.path().join("models");
    fs::create_dir_all(&models).unwrap();
    fs::File::create(models.join("huge.bin"))
        .and_then(|file| file.set_len((1024 << 20) + 1))
        .unwrap();
    fs::write(
        models.join("a.gltf"),
        br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"huge.bin","byteLength":4}]}"#,
    )
    .unwrap();
    fs::write(
        models.join("b.gltf"),
        br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"gone.bin","byteLength":4}]}"#,
    )
    .unwrap();

    let out = PackBuilder::new(proj.path(), dest.path())
        .build()
        .unwrap_or_else(|e| panic!("a file left out fails nothing: {e:#}"));

    let left_out: Vec<(&str, &LeftOutReason)> = out
        .left_out
        .iter()
        .map(|file| (file.path.as_str(), &file.reason))
        .collect();
    assert_eq!(left_out.len(), 2, "{left_out:?}");
    assert_eq!(left_out[0].0, "models/gone.bin");
    assert_eq!(left_out[0].1, &LeftOutReason::Missing);
    assert_eq!(left_out[1].0, "models/huge.bin");
    assert!(
        matches!(
            left_out[1].1,
            LeftOutReason::Refused(crate::trust::SecurityReason::TooLarge { .. })
        ),
        "{left_out:?}"
    );
    assert!(out.left_out[0].to_string().contains("models/gone.bin"));
}

/// **A build lists an external file whose path leaves the assets root.**
///
/// By `..` or as an absolute path, it is refused, left out and listed; the
/// build still succeeds.
#[test]
fn a_build_lists_an_external_file_leaving_the_assets_root() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let models = proj.path().join("models");
    fs::create_dir_all(&models).unwrap();
    fs::write(
        models.join("a.gltf"),
        br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"../../outside.bin","byteLength":4}]}"#,
    )
    .unwrap();
    fs::write(
        models.join("b.gltf"),
        br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"/abs/outside.bin","byteLength":4}]}"#,
    )
    .unwrap();

    let out = PackBuilder::new(proj.path(), dest.path())
        .build()
        .unwrap_or_else(|e| panic!("a file left out fails nothing: {e:#}"));

    let escapes = out
        .left_out
        .iter()
        .filter(|file| {
            matches!(
                file.reason,
                LeftOutReason::Refused(crate::trust::SecurityReason::Escape)
            )
        })
        .count();
    assert_eq!(escapes, 2, "{:?}", out.left_out);
}

/// **A build whose identity registry is refused fails, and says so.**
///
/// On its progress channel too: packed without it, a renamed asset would
/// ship under another identity.
#[test]
fn a_build_whose_registry_is_refused_fails_and_says_so() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let assets = proj.path().join("assets");
    seed_project(&assets);
    let registry = proj.path().join(REGISTRY_DIR).join(REGISTRY_FILE);
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::File::create(&registry)
        .and_then(|file| file.set_len((64 << 20) + 1))
        .unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
    let built = PackBuilder::new(&assets, dest.path())
        .with_progress(tx)
        .build();

    let error = format!(
        "{:#}",
        built.expect_err("a refused registry fails the build")
    );
    assert!(error.contains(REGISTRY_FILE), "{error}");
    assert!(
        rx.try_iter().any(|ev| matches!(
            ev,
            PackProgress::Failed { message } if message.contains(REGISTRY_FILE)
        )),
        "the failure is on the progress channel"
    );
}

/// **A build whose identity registry does not parse fails, and says so.**
#[test]
fn a_build_whose_registry_does_not_parse_fails_and_says_so() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let assets = proj.path().join("assets");
    seed_project(&assets);
    let registry = proj.path().join(REGISTRY_DIR).join(REGISTRY_FILE);
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, "<<<<<<< HEAD\n(entries: [])\n").unwrap();

    let built = PackBuilder::new(&assets, dest.path()).build();

    let error = format!(
        "{:#}",
        built.expect_err("an unread registry fails the build")
    );
    assert!(error.contains(REGISTRY_FILE), "{error}");
}

/// **A build that cannot make its destination says so on its progress channel.**
#[test]
fn a_build_that_cannot_write_reports_failed() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_project(proj.path());
    let not_a_folder = dest.path().join("taken");
    fs::write(&not_a_folder, b"a file").unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
    let built = PackBuilder::new(proj.path(), &not_a_folder)
        .with_progress(tx)
        .build();

    assert!(built.is_err());
    assert!(
        rx.try_iter()
            .any(|ev| matches!(ev, PackProgress::Failed { .. })),
        "the failure is on the progress channel"
    );
}

#[test]
fn empty_assets_root_produces_empty_pack() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();
    assert_eq!(out.asset_count, 0);
    assert_eq!(out.pack_bytes(), 0);
    // Both files exist but are zero-byte.
    assert!(out.data_pack.exists());
    assert!(out.index_bin.exists());
}

/// **The index build and the pack build agree on a script's dependencies**:
/// both name the UUID the pack indexes a renamed module under.
#[test]
fn a_packed_script_depends_on_the_uuid_its_renamed_module_is_packed_under() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let assets = proj.path().join("assets");
    fs::create_dir_all(assets.join("scripts")).unwrap();
    fs::write(
        assets.join("scripts").join("damage.erg"),
        b"fn void Hit() { }",
    )
    .unwrap();
    fs::write(
        assets.join("scripts").join("guard.erg"),
        br#"import "damage.erg"; fn void Main() { }"#,
    )
    .unwrap();
    let mut registry = AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
        proj.path(),
        crate::trust::Trust::Project,
        crate::trust::Refusals::detached(),
    ));
    registry.rename("scripts/hurt.erg", "scripts/damage.erg");
    registry.save().unwrap();

    let out = PackBuilder::new(&assets, dest.path()).build().unwrap();
    let packed = AssetIndex::new(&std::fs::read(&out.index_bin).unwrap()).unwrap();
    let developed = IndexBuilder::new(crate::trust::FileGuard::new(
        &assets,
        Trust::Project,
        Refusals::detached(),
    ))
    .with_registry(&AssetIdRegistry::load_or_unread(
        &crate::trust::FileGuard::new(
            proj.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ),
    ))
    .build_metadata()
    .unwrap();

    let in_pack = |key: &str| {
        packed
            .iter_all()
            .find(|m| khora_core::asset::asset_key(&m.source_path) == key)
            .unwrap_or_else(|| panic!("{key} is packed"))
            .clone()
    };
    let module = in_pack("scripts/damage.erg").uuid;
    let guard = in_pack("scripts/guard.erg");
    assert_eq!(module, AssetUUID::new_v5("scripts/hurt.erg"));
    assert_eq!(guard.dependencies, vec![module]);
    let developed_guard = developed
        .iter()
        .find(|m| m.uuid == guard.uuid)
        .expect("the index lists the script under the pack's UUID");
    assert_eq!(developed_guard.dependencies, guard.dependencies);
}

// ── Sub-assets ──────────────────────────────────────────────────────────

use std::any::Any;

use khora_core::asset::Stored;

use crate::asset::{AssetContainer, SubAssetEntry, SubAssetListerRegistration};

/// The kind name of the test-only container files packed here (`*.ksubpack`).
const PACK_CONTAINER_TYPE: &str = crate::asset::tests::kinds::PACK_CONTAINER.name();

/// Lists one sub-asset per non-empty line `"<type> <label>"`.
fn list_pack_container(bytes: &[u8]) -> Result<Vec<SubAssetEntry>> {
    let text = std::str::from_utf8(bytes)?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (type_name, label) = line
                .split_once(' ')
                .ok_or_else(|| anyhow!("malformed test container line: {line:?}"))?;
            Ok(SubAssetEntry {
                label: label.to_string(),
                type_name: type_name.to_string(),
            })
        })
        .collect()
}

inventory::submit! {
    SubAssetListerRegistration {
        type_name: PACK_CONTAINER_TYPE,
        list: list_pack_container,
    }
}

/// What the test container extracts: its label and the container's text.
#[derive(Debug)]
struct PackedSub {
    label: String,
    text: String,
}
impl Stored for PackedSub {}

struct TextContainer;

impl AssetContainer for TextContainer {
    fn open(&self, bytes: &[u8], _source_path: &Path) -> Result<Arc<dyn Any + Send + Sync>> {
        Ok(Arc::new(String::from_utf8(bytes.to_vec())?))
    }

    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        _container: AssetUUID,
        label: &str,
        _type_name: &str,
    ) -> Result<Box<dyn Any + Send>> {
        let text = opened
            .downcast_ref::<String>()
            .ok_or_else(|| anyhow!("not an opened text container"))?;
        Ok(Box::new(PackedSub {
            label: label.to_string(),
            text: text.clone(),
        }))
    }
}

const HERO: &[u8] = b"mesh mesh/Body\nmaterial material/Skin\n";

/// Returns the manifest of the pack `out`, once verified with `signer`'s key.
fn signed_manifest_of(out: &PackOutput, signer: &PackSigner) -> PackManifest {
    let path = (out.manifest_bin.as_ref()).expect("a signed build writes a manifest");
    let signed = SignedManifest::decode(&fs::read(path).unwrap()).unwrap();
    signed
        .verify(&signer.public_key(), &fs::read(&out.index_bin).unwrap())
        .unwrap()
}

fn seed_container_project(root: &Path) {
    fs::create_dir_all(root.join("models")).unwrap();
    fs::create_dir_all(root.join("textures")).unwrap();
    fs::write(root.join("models").join("hero.ksubpack"), HERO).unwrap();
    fs::write(root.join("textures").join("a.png"), b"PNG-A").unwrap();
}

#[test]
fn a_packed_index_keeps_sub_entries_and_packs_their_container_once() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_container_project(proj.path());

    let keys = tempdir().unwrap();
    let signer = PackSigner::load_or_create(keys.path()).unwrap();
    let out = PackBuilder::new(proj.path(), dest.path())
        .with_signer(&signer)
        .build()
        .unwrap();
    assert_eq!(
        out.pack_bytes(),
        (HERO.len() + b"PNG-A".len()) as u64,
        "the container's bytes are packed once, the subs add none"
    );

    let index_bytes = std::fs::read(&out.index_bin).unwrap();
    let vfs = AssetIndex::new(&index_bytes).unwrap();
    assert_eq!(vfs.asset_count(), 4, "two files and two subs");

    let hero = AssetUUID::new_v5("models/hero.ksubpack");
    assert!(matches!(
        vfs.get_metadata(&hero).and_then(|m| m.default_source()),
        Some(AssetSource::Packed { size, .. }) if *size == HERO.len() as u64
    ));
    for (label, type_name) in [("mesh/Body", "mesh"), ("material/Skin", "material")] {
        let sub = vfs
            .get_metadata(&AssetUUID::sub_asset(hero, label))
            .unwrap_or_else(|| panic!("sub '{label}' is in the packed index"));
        assert_eq!(sub.kind.name(), type_name);
        match sub.default_source() {
            Some(AssetSource::Sub {
                container,
                label: l,
            }) => {
                assert_eq!(*container, hero);
                assert_eq!(l, label);
            }
            other => panic!("sub '{label}' keeps its Sub source, got {other:?}"),
        }
    }

    let manifest = signed_manifest_of(&out, &signer);
    assert!(manifest.get(&hero).is_some(), "the container is hashed");
    assert!(
        manifest
            .get(&AssetUUID::sub_asset(hero, "mesh/Body"))
            .is_none(),
        "a sub has no bytes to hash"
    );
}

#[test]
fn a_packed_project_loads_a_sub_through_its_container() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_container_project(proj.path());

    let keys = tempdir().unwrap();
    let signer = PackSigner::load_or_create(keys.path()).unwrap();
    let out = PackBuilder::new(proj.path(), dest.path())
        .with_signer(&signer)
        .build()
        .unwrap();

    let index_bytes = std::fs::read(&out.index_bin).unwrap();
    let pack_file = std::fs::File::open(&out.data_pack).unwrap();
    let pack_loader = PackLoader::new(pack_file).expect("valid pack header");
    let manifest = signed_manifest_of(&out, &signer);
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc =
        AssetService::new(&index_bytes, Box::new(pack_loader), metrics, Some(manifest)).unwrap();
    svc.register_container(PACK_CONTAINER_TYPE, TextContainer);

    let hero = AssetUUID::new_v5("models/hero.ksubpack");
    let body = svc
        .load::<PackedSub>(&AssetUUID::sub_asset(hero, "mesh/Body"))
        .unwrap();
    assert_eq!(body.label, "mesh/Body");
    assert_eq!(body.text.as_bytes(), HERO);

    let skin = svc
        .load::<PackedSub>(&AssetUUID::sub_asset(hero, "material/Skin"))
        .unwrap();
    assert_eq!(skin.label, "material/Skin");

    // The other assets still round-trip.
    assert_eq!(
        svc.load_raw(&AssetUUID::new_v5("textures/a.png")).unwrap(),
        b"PNG-A"
    );
}

/// The pack header counts every index entry, sub entries included.
#[test]
fn the_pack_header_counts_every_index_entry_with_sub_entries() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_container_project(proj.path());

    let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();
    let vfs = AssetIndex::new(&std::fs::read(&out.index_bin).unwrap()).unwrap();
    let loader = PackLoader::new(std::fs::File::open(&out.data_pack).unwrap()).unwrap();
    assert_eq!(vfs.asset_count(), 4, "two files and two subs");
    assert_eq!(loader.header().asset_count, vfs.asset_count() as u32);
}

/// Progress counts the files streamed, one `Started` each, without sub entries.
#[test]
fn progress_counts_the_streamed_files_of_a_project_with_sub_entries() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    seed_container_project(proj.path());

    let (tx, rx) = crossbeam_channel::unbounded::<PackProgress>();
    PackBuilder::new(proj.path(), dest.path())
        .with_progress(tx)
        .build()
        .unwrap();

    let mut currents = Vec::new();
    let mut totals = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let PackProgress::Started { current, total, .. } = ev {
            currents.push(current);
            totals.push(total);
        }
    }
    assert_eq!(currents.len(), 2, "two files are streamed: {currents:?}");
    assert_eq!(
        totals,
        vec![2, 2],
        "the total is the number of streamed files"
    );
    assert_eq!(currents, vec![0, 1], "files are numbered 0..total");
}

/// **A container's external file is packed in the external-files table**; a README is not.
///
/// The index leaves the external file out, so the container reads it
/// from the table; a file of no asset no container reads is never packed.
#[test]
fn a_containers_external_file_is_packed_in_its_table() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    let models = proj.path().join("models");
    fs::create_dir_all(&models).unwrap();
    fs::write(
        models.join("a.gltf"),
        br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"a.bin","byteLength":4}]}"#,
    )
    .unwrap();
    fs::write(models.join("a.bin"), [7u8; 4]).unwrap();
    fs::write(proj.path().join("README.md"), b"# Assets").unwrap();

    let out = PackBuilder::new(proj.path(), dest.path()).build().unwrap();
    let vfs = AssetIndex::new(&fs::read(&out.index_bin).unwrap()).unwrap();
    let packed: Vec<String> = vfs
        .iter_all()
        .filter(|m| !matches!(m.default_source(), Some(AssetSource::Sub { .. })))
        .map(|m| asset_key(&m.source_path))
        .collect();
    assert_eq!(packed, ["models/a.gltf"], "only the glTF is an asset entry");
    let bin = vfs
        .external_file("models/a.bin")
        .expect("the buffer is in the external-files table");
    assert_eq!((bin.size, bin.uncompressed_size), (4, 4));
    assert!(
        vfs.external_file("README.md").is_none(),
        "the README is not packed"
    );
}

/// The header's LZ4 flag is set only when some entry is LZ4-compressed.
#[test]
fn the_lz4_flag_is_clear_when_no_entry_compresses() {
    let proj = tempdir().unwrap();
    let dest = tempdir().unwrap();
    // Five bytes LZ4 cannot shrink: each entry is written uncompressed.
    fs::write(proj.path().join("a.png"), b"ABCDE").unwrap();

    let out = PackBuilder::new(proj.path(), dest.path())
        .with_compression(CompressionKind::Lz4)
        .build()
        .unwrap();
    let vfs = AssetIndex::new(&fs::read(&out.index_bin).unwrap()).unwrap();
    let meta = vfs.iter_all().next().unwrap();
    let Some(AssetSource::Packed { compression, .. }) = meta.default_source() else {
        panic!("a packed entry");
    };
    assert_eq!(
        *compression,
        CompressionKind::None,
        "the entry is stored raw"
    );

    let loader = PackLoader::new(fs::File::open(&out.data_pack).unwrap()).unwrap();
    assert_eq!(
        loader.header().flags & PACK_FLAG_LZ4,
        0,
        "no entry is LZ4-compressed, yet the header says one is"
    );
}
