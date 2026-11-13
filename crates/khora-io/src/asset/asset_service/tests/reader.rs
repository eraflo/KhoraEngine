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

//! The shared reader: decoding off the service's lock, across threads and
//! index replacements, without touching the service's caches.

use super::*;

#[test]
fn a_reader_decodes_a_file_to_the_value_load_returns() {
    let mut svc = shared_service(shared_files());
    let decoded = svc.reader().decode(&note(), note_type());

    assert_eq!(decoded.uuid, note(), "the decode names its asset");
    assert_eq!(decoded.type_id, note_type(), "and the type asked for");
    assert!(
        decoded.external_files.is_empty(),
        "a note is no container: {:?}",
        decoded.external_files
    );
    let from_reader = decoded_as::<TestNote>(decoded, "the note");
    let from_load = svc.load::<TestNote>(&note()).unwrap();
    assert_eq!(from_reader.text, "hello");
    assert_eq!(from_reader.text, from_load.text);
}

#[test]
fn a_reader_decode_touches_none_of_the_services_caches() {
    let mut svc = shared_service(shared_files());
    let decodes = Arc::new(AtomicUsize::new(0));
    svc.register_decoder::<TestNote>(
        NOTE_TYPE,
        CountingNoteDecoder {
            decodes: decodes.clone(),
        },
    );
    let reader = svc.reader();

    decoded_as::<TestNote>(reader.decode(&note(), note_type()), "the first decode");
    decoded_as::<TestNote>(reader.decode(&note(), note_type()), "the second decode");
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        2,
        "decoding one asset twice decodes it twice"
    );
    assert_eq!(svc.load_count(), 0, "a reader decode is no load");
    assert_eq!(svc.cached_type_count(), 0, "a reader decode caches nothing");

    svc.load::<TestNote>(&note()).unwrap();
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        3,
        "the service's load decodes: the reader filled no cache for it"
    );
    svc.load::<TestNote>(&note()).unwrap();
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        3,
        "the service still serves its own cache"
    );
    assert_eq!(svc.load_count(), 1);

    decoded_as::<TestNote>(reader.decode(&note(), note_type()), "a decode after a load");
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        4,
        "the service's cache does not serve the reader"
    );
}

#[test]
fn a_reader_decode_of_an_unknown_uuid_is_an_error_value() {
    let svc = shared_service(shared_files());
    let missing = AssetUUID::new_v5("missing");
    let decoded = svc.reader().decode(&missing, note_type());

    assert_eq!(decoded.uuid, missing);
    let err = decoded
        .value
        .expect_err("an unindexed asset does not decode");
    assert!(!err.is_empty(), "the error says why");
    assert!(decoded.external_files.is_empty());
}

#[test]
fn a_failed_reader_decode_is_not_remembered_as_a_failed_load() {
    let svc = shared_service(shared_files());
    let missing = AssetUUID::new_v5("missing");
    assert!(svc.reader().decode(&missing, note_type()).value.is_err());
    assert!(
        !svc.has_failed(&missing),
        "only the service's own load records a failure"
    );
}

#[test]
fn a_reader_decode_whose_bytes_do_not_decode_is_an_error_value() {
    let files = shared_files();
    files
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(NOTE_PATH), vec![0xFF, 0xFE, 0xFD]);
    let svc = shared_service(files);
    let reader = svc.reader();

    assert!(
        reader.decode(&note(), note_type()).value.is_err(),
        "bytes that are no UTF-8 note"
    );
    assert!(
        reader.decode(&villain(), note_type()).value.is_err(),
        "no decoder is registered for the container file's type"
    );
}

#[test]
fn a_reader_decode_as_another_type_is_an_error_value() {
    let svc = shared_service(shared_files());
    let reader = svc.reader();

    assert!(
        reader
            .decode(&note(), TypeId::of::<TestMesh>())
            .value
            .is_err(),
        "a note is no TestMesh"
    );
    assert!(
        reader
            .decode(&hero_body(), TypeId::of::<TestMaterial>())
            .value
            .is_err(),
        "a mesh part is no material"
    );
    let body = decoded_as::<TestMesh>(
        reader.decode(&hero_body(), TypeId::of::<TestMesh>()),
        "the body",
    );
    assert_eq!(body.label, "mesh/Body", "it still decodes as its own type");
}

#[test]
fn a_reader_decodes_a_sub_to_the_value_load_returns() {
    let (mut svc, _) = container_service(HERO_BYTES, None);
    let reader = svc.reader();

    let body = decoded_as::<TestMesh>(
        reader.decode(&hero_body(), TypeId::of::<TestMesh>()),
        "the body",
    );
    let loaded = svc.load::<TestMesh>(&hero_body()).unwrap();
    assert_eq!(body.label, loaded.label);
    assert_eq!(body.container, loaded.container);
    assert_eq!(body.source_path, loaded.source_path);
    assert_eq!(body.text, loaded.text);
    assert_eq!(body.container, hero());
    assert_eq!(body.text.as_bytes(), HERO_BYTES);

    let skin = decoded_as::<TestMaterial>(
        reader.decode(&hero_skin(), TypeId::of::<TestMaterial>()),
        "the skin",
    );
    assert_eq!(skin.label, "material/Skin");
    assert_eq!(
        skin.label,
        svc.load::<TestMaterial>(&hero_skin()).unwrap().label
    );
}

#[test]
fn a_reader_decode_of_a_sub_whose_container_fails_its_integrity_check_is_an_error_value() {
    let mut manifest = PackManifest::new();
    manifest.insert(hero(), HERO_BYTES);
    manifest.insert(villain(), VILLAIN_BYTES);
    let (svc, opens) = container_service(b"TAMPERED-HERO!", Some(manifest));

    let err = svc
        .reader()
        .decode(&hero_body(), TypeId::of::<TestMesh>())
        .value
        .expect_err("tampered container bytes must fail the integrity check");
    assert!(
        err.contains("integrity") || err.contains("BLAKE3") || err.contains("size mismatch"),
        "expected an integrity error, got: {err}"
    );
    assert_eq!(
        opens.load(Ordering::SeqCst),
        0,
        "tampered bytes never reach the container"
    );
}

#[test]
fn a_reader_reports_the_files_a_container_read_besides_its_own() {
    let mut svc = AssetService::new(
        &container_index_bytes(),
        Box::new(MockIo {
            files: container_files(HERO_BYTES),
        }),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .unwrap();
    svc.register_container(
        CONTAINER_TYPE,
        ReadingBesideContainer(CountingContainer {
            opens: Arc::new(AtomicUsize::new(0)),
        }),
    );
    // The container file decodes whole too, as a glTF decodes as one mesh.
    svc.register_decoder::<TestNote>(CONTAINER_TYPE, NoteDecoder);
    let reader = svc.reader();

    let part = reader.decode(&hero_body(), TypeId::of::<TestMesh>());
    assert_eq!(
        part.external_files,
        vec![(hero(), vec![SKIN_KEY.to_string()])],
        "a part names its container and the files the container read"
    );
    decoded_as::<TestMesh>(part, "the body");

    let whole = reader.decode(&hero(), note_type());
    assert_eq!(
        whole.external_files,
        vec![(hero(), vec![SKIN_KEY.to_string()])],
        "a container decoded whole names the files it reads beside it"
    );
    assert_eq!(
        decoded_as::<TestNote>(whole, "the whole hero")
            .text
            .as_bytes(),
        HERO_BYTES
    );
}

#[test]
fn a_reader_taken_before_a_reindex_keeps_reading_the_index_it_was_taken_with() {
    let files = shared_files();
    files
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(LATER_PATH), b"later".to_vec());
    let mut svc = shared_service(files);
    let before = svc.reader();

    svc.reindex(&index_of(vec![note_entry(LATER_PATH)]))
        .unwrap();

    assert_eq!(
        decoded_as::<TestNote>(before.decode(&note(), note_type()), "the note").text,
        "hello",
        "the old index still lists the note"
    );
    assert!(
        before.decode(&later(), note_type()).value.is_err(),
        "the old index does not list the later note"
    );
}

#[test]
fn the_reader_taken_after_a_reindex_reads_the_new_index() {
    let files = shared_files();
    files
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(LATER_PATH), b"later".to_vec());
    let mut svc = shared_service(files);
    // A reader still held elsewhere does not keep the service on its index.
    let _held = svc.reader();

    svc.reindex(&index_of(vec![note_entry(LATER_PATH)]))
        .unwrap();
    let after = svc.reader();

    assert_eq!(
        decoded_as::<TestNote>(after.decode(&later(), note_type()), "the later note").text,
        "later"
    );
    assert!(
        after.decode(&note(), note_type()).value.is_err(),
        "the new index no longer lists the note"
    );
    assert_eq!(svc.load::<TestNote>(&later()).unwrap().text, "later");
    assert!(
        svc.load::<TestNote>(&note()).is_err(),
        "the service's load agrees with its new reader"
    );
}

/// The editor's `share_assets` swaps a project's service into the
/// engine's: the reader the engine's service hands out from then on reads
/// the swapped-in project.
#[test]
fn a_service_swapped_in_hands_out_the_reader_of_its_own_project() {
    let mut own = shared_service(shared_files());
    let other_files = shared_files();
    other_files
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(NOTE_PATH), b"other".to_vec());
    let mut target = shared_service(other_files);

    std::mem::swap(&mut own, &mut target);
    target.announce_replacing(&own);

    assert_eq!(
        decoded_as::<TestNote>(target.reader().decode(&note(), note_type()), "target").text,
        "hello"
    );
    assert_eq!(
        decoded_as::<TestNote>(own.reader().decode(&note(), note_type()), "own").text,
        "other"
    );
}

#[test]
fn a_reader_decodes_on_another_thread_while_the_service_serves_cached_loads() {
    let mut svc = shared_service(shared_files());
    let cached = svc.load::<TestNote>(&note()).unwrap();
    let reader = svc.reader();

    // Moved to a thread of its own: the reader is `Send + 'static` and
    // needs nothing of the service the main thread keeps using.
    let worker = std::thread::spawn(move || {
        (0..50)
            .map(|_| decoded_as::<TestNote>(reader.decode(&note(), note_type()), "note").text)
            .collect::<Vec<_>>()
    });
    for _ in 0..50 {
        assert_eq!(svc.load::<TestNote>(&note()).unwrap(), cached);
    }
    let texts = worker.join().expect("the reader's thread completes");

    assert!(texts.iter().all(|text| text == "hello"), "{texts:?}");
    assert_eq!(svc.load_count(), 1, "the reader's decodes are no loads");
}

#[test]
fn the_reader_is_shared_across_threads_and_its_results_sent_between_them() {
    fn shared<T: Send + Sync + 'static>() {}
    fn sent<T: Send + 'static>() {}
    shared::<AssetReader>();
    shared::<Arc<AssetReader>>();
    sent::<Decoded>();
}

/// A decoder registered while a reader is held elsewhere still reaches the
/// service's own loads, and the reader taken after it.
#[test]
fn a_decoder_registered_while_a_reader_is_held_serves_the_services_loads() {
    let mut svc = shared_service(shared_files());
    let held = svc.reader();
    let decodes = Arc::new(AtomicUsize::new(0));
    svc.register_decoder::<TestNote>(
        NOTE_TYPE,
        CountingNoteDecoder {
            decodes: decodes.clone(),
        },
    );

    svc.load::<TestNote>(&note()).unwrap();
    assert_eq!(decodes.load(Ordering::SeqCst), 1, "the load used it");
    decoded_as::<TestNote>(svc.reader().decode(&note(), note_type()), "a new reader");
    assert_eq!(decodes.load(Ordering::SeqCst), 2, "the new reader uses it");
    decoded_as::<TestNote>(held.decode(&note(), note_type()), "the held reader");
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        2,
        "the reader held before keeps the decoders it was taken with"
    );
}

/// A container file loaded as a type its decoder does not produce fails
/// before its container is opened: the load records no file read beside
/// it, so a change to such a file does not announce the container.
#[test]
fn a_container_file_loaded_as_another_type_records_no_file_read_beside_it() {
    let mut svc = AssetService::new(
        &container_index_bytes(),
        Box::new(MockIo {
            files: container_files(HERO_BYTES),
        }),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    svc.register_container(
        CONTAINER_TYPE,
        ReadingBesideContainer(CountingContainer {
            opens: opens.clone(),
        }),
    );
    svc.register_decoder::<TestNote>(CONTAINER_TYPE, NoteDecoder);

    assert!(
        svc.load::<TestMesh>(&hero()).is_err(),
        "the hero file decodes to a note, not a TestMesh"
    );
    svc.invalidate(&AssetUUID::new_v5(SKIN_KEY));
    let changed = svc.take_changes();
    assert!(
        !changed.contains(&hero()),
        "a failed load recorded the files its container reads: {changed:?}"
    );
    assert_eq!(
        opens.load(Ordering::SeqCst),
        0,
        "a load as another type never opens the container"
    );
}

/// **An empty service serves nothing**: no asset indexed, no project
/// served, and any id it is asked for is refused.
#[test]
fn an_empty_service_serves_no_asset_and_no_project() {
    let mut svc = AssetService::empty();

    assert_eq!(svc.index().asset_count(), 0, "no asset is indexed");
    assert_eq!(svc.project_root(), None, "no project is served");
    assert!(
        svc.load_raw(&AssetUUID::new_v5("scenes/default.kscene"))
            .is_err(),
        "an id no index holds is refused"
    );
}
