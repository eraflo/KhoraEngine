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

//! Change announcements: what invalidating an asset or replacing a service
//! names on the change stream, overflow included.

use super::*;

#[test]
fn invalidating_an_asset_announces_it() {
    let mut svc = shared_service(shared_files());
    svc.load::<TestNote>(&note()).unwrap();

    svc.invalidate(&note());

    assert_eq!(svc.changes().drain(), vec![AssetChanged(note())]);
}

#[test]
fn invalidating_an_asset_nothing_cached_still_announces_it() {
    let mut svc = shared_service(shared_files());

    assert!(
        !svc.invalidate(&note()),
        "a never-loaded asset has nothing to drop"
    );

    assert_eq!(
        svc.changes().drain(),
        vec![AssetChanged(note())],
        "a copy of it may still live outside the service"
    );
}

#[test]
fn two_invalidations_before_a_drain_announce_once() {
    let mut svc = shared_service(shared_files());
    svc.load::<TestNote>(&note()).unwrap();

    svc.invalidate(&note());
    svc.invalidate(&note());

    assert_eq!(svc.changes().drain(), vec![AssetChanged(note())]);
}

#[test]
fn invalidating_a_container_announces_each_part_it_served() {
    let mut svc = shared_service(shared_files());
    svc.load::<TestMesh>(&hero_body()).unwrap();
    svc.load::<TestMaterial>(&hero_skin()).unwrap();
    svc.load::<TestMesh>(&villain_body()).unwrap();

    svc.invalidate(&hero());

    let announced = svc.changes().drain();
    assert_eq!(announced.len(), 3, "no uuid twice: {announced:?}");
    assert_eq!(
        announced.into_iter().map(|c| c.0).collect::<HashSet<_>>(),
        HashSet::from([hero(), hero_body(), hero_skin()]),
        "the container and each of its parts, not another container's"
    );
}

/// After a failed reload, the next invalidation of the container still names its parts.
///
/// The engine still holds the copies it took before.
#[test]
fn a_container_still_announces_its_parts_after_a_failed_reload() {
    let files = shared_files();
    let mut svc = shared_service(files.clone());
    svc.load::<TestMesh>(&hero_body()).unwrap();
    svc.invalidate(&hero());
    assert_eq!(
        drained_set(&svc),
        HashSet::from([hero(), hero_body()]),
        "the first invalidation names the container and its part"
    );

    // The re-exported file is broken: the container does not open.
    files
        .lock()
        .unwrap()
        .insert(std::path::PathBuf::from(HERO_PATH), vec![0xFF, 0xFE, 0xFD]);
    assert!(svc.load::<TestMesh>(&hero_body()).is_err());

    svc.invalidate(&hero());

    assert_eq!(
        drained_set(&svc),
        HashSet::from([hero(), hero_body()]),
        "the part is announced again after the failed reload"
    );
}

#[test]
fn announce_replacing_names_every_asset_the_outgoing_service_served() {
    let mut outgoing = shared_service(shared_files());
    outgoing.load::<TestNote>(&note()).unwrap();
    outgoing.load::<TestMesh>(&hero_body()).unwrap();

    let incoming = shared_service(shared_files());
    incoming.announce_replacing(&outgoing);

    assert_eq!(
        drained_set(&incoming),
        HashSet::from([note(), hero_body()]),
        "the incoming service announces what the outgoing one served"
    );
    assert!(
        outgoing.changes().drain().is_empty(),
        "the outgoing service's own stream is not the one written to"
    );
}

/// Replacing a service that served more assets than the stream holds still names each.
///
/// The announcements that do not fit are lost, yet the engine holds a copy of
/// every asset the outgoing service served.
#[test]
fn a_replacement_overflowing_the_stream_still_names_everything_the_outgoing_service_served() {
    let count = CHANGE_BACKLOG + 1;
    let paths: Vec<String> = (0..count).map(|i| format!("notes/{i}.ktestnote")).collect();
    let mut files = HashMap::new();
    let metadata: Vec<AssetMetadata> = paths
        .iter()
        .map(|path| {
            files.insert(std::path::PathBuf::from(path), b"note".to_vec());
            let mut entry = file_entry(AssetUUID::new_v5(path), path);
            entry.asset_type_name = NOTE_TYPE.to_string();
            entry
        })
        .collect();
    let index = bincode::serde::encode_to_vec(&metadata, bincode::config::standard()).unwrap();
    let mut outgoing = AssetService::new(
        &index,
        Box::new(SharedIo {
            files: Arc::new(Mutex::new(files)),
        }),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .unwrap();
    outgoing.register_decoder::<TestNote>(NOTE_TYPE, NoteDecoder);
    for path in &paths {
        outgoing.load::<TestNote>(&AssetUUID::new_v5(path)).unwrap();
    }

    let incoming = shared_service(shared_files());
    incoming.announce_replacing(&outgoing);
    let taken = incoming.take_changes();

    let missing: Vec<&String> = paths
        .iter()
        .filter(|path| !taken.contains(&AssetUUID::new_v5(path)))
        .collect();
    assert!(
        missing.is_empty(),
        "every asset the outgoing service served is handed over: {} of {count} missing, \
         e.g. {:?}",
        missing.len(),
        missing.first()
    );
}
