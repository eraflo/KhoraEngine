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

//! Sub-assets: extracted from their container, opened once, dropped with
//! it, and refused when their container cannot serve them.

use super::*;

#[test]
fn loading_a_sub_returns_the_value_its_container_extracts() {
    let (mut svc, _) = container_service(HERO_BYTES, None);
    let body = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(body.label, "mesh/Body");
    assert_eq!(body.container, hero());
    assert_eq!(body.source_path, std::path::PathBuf::from(HERO_PATH));
    assert_eq!(body.text.as_bytes(), HERO_BYTES);

    let skin = svc.load::<TestMaterial>(&hero_skin()).unwrap();
    assert_eq!(skin.label, "material/Skin");
}

#[test]
fn loading_two_subs_of_one_container_opens_it_once() {
    let (mut svc, opens) = container_service(HERO_BYTES, None);
    svc.load::<TestMesh>(&hero_body()).unwrap();
    svc.load::<TestMaterial>(&hero_skin()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 1, "one open for both subs");

    // A second load of a sub is served from the cache.
    let first = svc.load::<TestMesh>(&hero_body()).unwrap();
    let again = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(first, again, "a loaded sub is cached under its uuid");
    assert_eq!(opens.load(Ordering::SeqCst), 1);

    // Another container is opened on its own.
    svc.load::<TestMesh>(&villain_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 2);
}

#[test]
fn loading_a_sub_as_the_wrong_type_errors() {
    let (mut svc, _) = container_service(HERO_BYTES, None);
    let err = svc
        .load::<TestMaterial>(&hero_body())
        .expect_err("a mesh sub cannot load as a material");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("TestMaterial") || msg.contains("mesh"),
        "the error names the type: {msg}"
    );
    // The sub still loads as its own type afterwards.
    let body = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(body.label, "mesh/Body");
}

/// A sub has no bytes of its own: integrity is checked on the container's
/// bytes, under the container's uuid (the pack manifest has no sub entry).
#[test]
fn a_sub_is_verified_against_the_manifest_under_its_container_uuid() {
    let mut manifest = PackManifest::new();
    manifest.insert(hero(), HERO_BYTES);
    manifest.insert(villain(), VILLAIN_BYTES);

    let (mut svc, _) = container_service(HERO_BYTES, Some(manifest.clone()));
    let body = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(body.text.as_bytes(), HERO_BYTES);

    let (mut svc_bad, opens) = container_service(b"TAMPERED-HERO!", Some(manifest));
    let err = svc_bad
        .load::<TestMesh>(&hero_body())
        .expect_err("tampered container bytes must fail the integrity check");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("integrity") || msg.contains("BLAKE3") || msg.contains("size mismatch"),
        "expected integrity error, got: {msg}"
    );
    assert_eq!(
        opens.load(Ordering::SeqCst),
        0,
        "tampered bytes never reach the container"
    );
}

#[test]
fn load_raw_of_a_sub_errors() {
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &container_index_bytes(),
        Box::new(MockIo {
            files: container_files(HERO_BYTES),
        }),
        metrics,
        None,
    )
    .unwrap();
    let err = svc
        .load_raw(&hero_body())
        .expect_err("a sub-asset has no bytes of its own");
    let msg = format!("{err:#}");
    assert!(msg.contains("sub-asset"), "unexpected error: {msg}");
    // The container's own bytes are still readable.
    assert_eq!(svc.load_raw(&hero()).unwrap(), HERO_BYTES);
}

#[test]
fn invalidating_the_container_drops_its_opened_container_and_subs() {
    let (mut svc, opens) = container_service(HERO_BYTES, None);
    let before = svc.load::<TestMesh>(&hero_body()).unwrap();
    svc.load::<TestMesh>(&villain_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 2);

    assert!(
        svc.invalidate(&hero()),
        "invalidating a container with cached subs drops something"
    );

    // The hero is opened again; its sub is a fresh value.
    let after = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 3, "the container re-opens");
    assert_ne!(before, after, "the cached sub was dropped");

    // The villain's sub and opened container are untouched.
    svc.load::<TestMesh>(&villain_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 3);
}

#[test]
fn invalidating_an_opened_container_without_cached_subs_returns_true() {
    let (mut svc, opens) = container_service(HERO_BYTES, None);
    svc.load::<TestMesh>(&hero_body()).unwrap();
    // Drop the sub alone: the opened container stays cached.
    assert!(svc.invalidate(&hero_body()));
    svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 1, "the container stays open");
    assert!(svc.invalidate(&hero_body()));

    // Only the opened container is left: dropping it is still a drop.
    assert!(svc.invalidate(&hero()));
    assert!(
        !svc.invalidate(&hero()),
        "nothing is left to drop the second time"
    );
    svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(opens.load(Ordering::SeqCst), 2, "the container re-opens");
}

/// Subs of one container cached under different Rust types are all
/// dropped with it.
#[test]
fn invalidating_the_container_drops_its_subs_of_every_type() {
    let (mut svc, _) = container_service(HERO_BYTES, None);
    let body = svc.load::<TestMesh>(&hero_body()).unwrap();
    let skin = svc.load::<TestMaterial>(&hero_skin()).unwrap();

    assert!(svc.invalidate(&hero()));

    assert_ne!(
        body,
        svc.load::<TestMesh>(&hero_body()).unwrap(),
        "the cached mesh sub was dropped"
    );
    assert_ne!(
        skin,
        svc.load::<TestMaterial>(&hero_skin()).unwrap(),
        "the cached material sub was dropped"
    );
}

/// Invalidating a container drops a sub the rebuilt index has dropped.
///
/// It is a sub of that container whatever the new index says, so it stops
/// being served.
#[test]
fn invalidating_a_container_after_a_reindex_drops_a_sub_the_new_index_dropped() {
    let (mut svc, _) = container_service(HERO_BYTES, None);
    svc.load::<TestMesh>(&hero_body()).unwrap();

    // The re-exported hero holds the skin only.
    let reexported = vec![
        file_entry(hero(), HERO_PATH),
        sub_entry(hero_skin(), hero(), HERO_PATH, "material/Skin", "material"),
        file_entry(villain(), VILLAIN_PATH),
        sub_entry(villain_body(), villain(), VILLAIN_PATH, "mesh/Body", "mesh"),
    ];
    svc.reindex(&bincode::serde::encode_to_vec(&reexported, bincode::config::standard()).unwrap())
        .unwrap();
    svc.invalidate(&hero());

    assert!(
        svc.load::<TestMesh>(&hero_body()).is_err(),
        "a sub of the invalidated container that the index no longer lists \
         must not be served from the cache"
    );
}

#[test]
fn loading_a_sub_without_a_registered_container_errors() {
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &container_index_bytes(),
        Box::new(MockIo {
            files: container_files(HERO_BYTES),
        }),
        metrics,
        None,
    )
    .unwrap();
    assert!(
        svc.load::<TestMesh>(&hero_body()).is_err(),
        "no container opens this file type"
    );
}

#[test]
fn loading_a_sub_whose_container_is_not_indexed_errors() {
    let orphan = AssetUUID::new_v5("test-sub/orphan/mesh/Body");
    let metadata = vec![sub_entry(
        orphan,
        AssetUUID::new_v5("models/gone.ksubtest"),
        "models/gone.ksubtest",
        "mesh/Body",
        "mesh",
    )];
    let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
    let metrics = Arc::new(MetricsRegistry::new());
    let mut svc = AssetService::new(
        &index,
        Box::new(MockIo {
            files: HashMap::new(),
        }),
        metrics,
        None,
    )
    .unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    svc.register_container(
        CONTAINER_TYPE,
        CountingContainer {
            opens: opens.clone(),
        },
    );
    assert!(svc.load::<TestMesh>(&orphan).is_err());
    assert_eq!(opens.load(Ordering::SeqCst), 0);
}
