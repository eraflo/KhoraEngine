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

//! Loading through the index and the IO: raw bytes, a replaced index, a
//! failed load remembered, and integrity checked against a manifest.

use super::*;

#[test]
fn vfs_accessor_returns_underlying_vfs() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("textures")).unwrap();
    fs::write(dir.path().join("textures").join("a.png"), b"PNG").unwrap();
    let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
    let metrics = Arc::new(MetricsRegistry::new());
    let svc = AssetService::new(
        &bytes,
        Box::new(MockIo {
            files: HashMap::new(),
        }),
        metrics,
        None,
    )
    .unwrap();
    assert_eq!(svc.index().asset_count(), 1);
}

#[test]
fn load_raw_returns_bytes_without_decode() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("scenes")).unwrap();
    fs::write(dir.path().join("scenes").join("a.kscene"), b"SCN").unwrap();
    let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();

    let mut files = HashMap::new();
    files.insert(std::path::PathBuf::from("scenes/a.kscene"), b"SCN".to_vec());
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(&bytes, Box::new(MockIo { files }), metrics, None).unwrap();

    let uuid = AssetUUID::new_v5("scenes/a.kscene");
    let raw = svc.load_raw(&uuid).unwrap();
    assert_eq!(raw, b"SCN");
}

#[test]
fn reindex_swaps_vfs() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("textures")).unwrap();
    fs::write(dir.path().join("textures").join("a.png"), b"PNG").unwrap();
    let bytes_a = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &bytes_a,
        Box::new(MockIo {
            files: HashMap::new(),
        }),
        metrics,
        None,
    )
    .unwrap();
    assert_eq!(svc.index().asset_count(), 1);

    // Add another asset, rebuild, reindex.
    fs::write(dir.path().join("textures").join("b.png"), b"PNG2").unwrap();
    let bytes_b = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
    svc.reindex(&bytes_b).unwrap();
    assert_eq!(svc.index().asset_count(), 2);

    let new_uuid = AssetUUID::new_v5("textures/b.png");
    assert!(svc.index().get_metadata(&new_uuid).is_some());
}

#[test]
fn invalidate_returns_false_when_nothing_cached() {
    let dir = tempdir().unwrap();
    let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &bytes,
        Box::new(MockIo {
            files: HashMap::new(),
        }),
        metrics,
        None,
    )
    .unwrap();
    assert!(!svc.invalidate(&AssetUUID::new_v5("missing")));
}

#[test]
fn a_failed_load_is_remembered_until_an_asset_changes_or_the_index_is_replaced() {
    let dir = tempdir().unwrap();
    let bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();
    let mut svc = AssetService::new(
        &bytes,
        Box::new(MockIo {
            files: HashMap::new(),
        }),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .unwrap();
    let missing = AssetUUID::new_v5("missing");
    assert!(!svc.has_failed(&missing), "never tried");

    assert!(svc
        .load::<khora_core::renderer::api::resource::CpuTexture>(&missing)
        .is_err());
    assert!(svc.has_failed(&missing));

    svc.invalidate(&AssetUUID::new_v5("another"));
    assert!(!svc.has_failed(&missing), "any change forgets the failure");

    assert!(svc
        .load::<khora_core::renderer::api::resource::CpuTexture>(&missing)
        .is_err());
    svc.reindex(&bytes).unwrap();
    assert!(!svc.has_failed(&missing), "a new index forgets the failure");
}

#[test]
fn load_raw_verifies_against_manifest_and_rejects_corruption() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("scenes")).unwrap();
    let payload = b"SCENE-PAYLOAD";
    fs::write(dir.path().join("scenes").join("a.kscene"), payload).unwrap();
    let index_bytes = IndexBuilder::new(dir.path()).build_index_bytes().unwrap();

    let uuid = AssetUUID::new_v5("scenes/a.kscene");

    // Build a manifest matching the genuine payload.
    let mut manifest = PackManifest::new();
    manifest.insert(uuid, payload);

    // Mock returns the genuine payload — verification should pass.
    let mut files = HashMap::new();
    files.insert(
        std::path::PathBuf::from("scenes/a.kscene"),
        payload.to_vec(),
    );
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &index_bytes,
        Box::new(MockIo { files }),
        metrics,
        Some(manifest.clone()),
    )
    .unwrap();
    let raw = svc.load_raw(&uuid).unwrap();
    assert_eq!(raw, payload);

    // Same uuid and manifest, other bytes: verification fails.
    let mut bad_files = HashMap::new();
    bad_files.insert(
        std::path::PathBuf::from("scenes/a.kscene"),
        b"CORRUPTED-PAY".to_vec(),
    );
    let metrics2 = Arc::new(MetricsRegistry::new());
    let mut svc_bad = AssetService::new(
        &index_bytes,
        Box::new(MockIo { files: bad_files }),
        metrics2,
        Some(manifest),
    )
    .unwrap();
    let err = svc_bad.load_raw(&uuid).unwrap_err();
    let msg = format!("{:#}", err);
    assert!(
        msg.contains("integrity") || msg.contains("BLAKE3") || msg.contains("size mismatch"),
        "expected integrity error, got: {}",
        msg
    );
}
