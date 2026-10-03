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

//! `FastestLoad` through the serialization service: a snapshot written, and
//! read back by `load_world` and `replace_world` from its header alone —
//! atomically, a damaged one leaving the world as it was.

use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SerializationGoal, SCENE_FORMAT_VERSION};
use khora_data::ecs::{Name, Parent, Transform, World};
use khora_data::scene::registration_of;
use khora_io::serialization::SerializationService;

/// One entity as a test sees it: identity, name, translation, parent's
/// identity.
type Observed = (
    PersistentId,
    Option<String>,
    Option<Vec3>,
    Option<PersistentId>,
);

/// What a test can observe of a world, sorted by identity.
fn observed(world: &World) -> Vec<Observed> {
    let mut seen: Vec<_> = world
        .iter_entities()
        .map(|e| {
            (
                world.persistent_id(e).expect("every entity has an id"),
                world.get::<Name>(e).map(|n| n.as_str().to_owned()),
                world.get::<Transform>(e).map(|t| t.translation),
                world
                    .get::<Parent>(e)
                    .and_then(|p| world.persistent_id(p.0)),
            )
        })
        .collect();
    seen.sort_by_key(|(id, ..)| *id);
    seen
}

/// A small authored scene: a parent and two children, with names.
fn authored_world(prefix: &str) -> World {
    let mut world = World::new();
    let root = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new(format!("{prefix} root")),
    ));
    for (n, x) in [(1, 0.5), (2, -0.5)] {
        let child = world.spawn((
            Transform::from_translation(Vec3::new(x, 0.1, 0.0)),
            Name::new(format!("{prefix} child {n}")),
        ));
        world.set_parent(child, Some(root));
        world.mark_authored(child).expect("authored");
    }
    world.mark_authored(root).expect("authored");
    world
}

/// The encoding a file's header names.
fn encoding_of(file: &SceneFile) -> &str {
    std::str::from_utf8(&file.header.encoding_id)
        .expect("the encoding id is UTF-8")
        .trim_end_matches('\0')
}

/// `FastestLoad` as a file on disk: saved, written out and read back — a
/// snapshot, so what reads it is the snapshot path.
fn fastest(world: &World) -> SceneFile {
    let file = SerializationService::new()
        .save_world(world, SerializationGoal::FastestLoad)
        .unwrap_or_else(|e| panic!("the snapshot save failed: {e:?}"));
    let file = SceneFile::from_bytes(&file.to_bytes()).expect("the file parses");
    assert_eq!(
        encoding_of(&file),
        "KH_SNAPSHOT_V1",
        "FastestLoad is a snapshot"
    );
    file
}

/// `file` with its payload replaced, the header's length kept in step.
fn with_payload(file: &SceneFile, payload: Vec<u8>) -> SceneFile {
    let mut header = file.header.clone();
    header.payload_length = payload.len() as u64;
    SceneFile { header, payload }
}

/// `FastestLoad` writes the snapshot encoding behind a current header.
#[test]
fn fastest_load_writes_a_snapshot() {
    let file = fastest(&authored_world("Saved"));
    assert_eq!(encoding_of(&file), "KH_SNAPSHOT_V1");
    assert_eq!(file.header.format_version, SCENE_FORMAT_VERSION);
}

/// `load_world` reads a snapshot back beside what the world holds, under the
/// saved identities.
#[test]
fn load_world_reads_a_snapshot() {
    let service = SerializationService::new();
    let source = authored_world("Saved");
    let file = fastest(&source);

    let mut world = World::new();
    let report = service
        .load_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the snapshot did not load: {e:?}"));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(observed(&world), observed(&source));

    let mut beside = authored_world("Resident");
    let resident = observed(&beside);
    service
        .load_world(&file, &mut beside)
        .unwrap_or_else(|e| panic!("the snapshot did not load beside: {e:?}"));
    let mut expected = [resident, observed(&source)].concat();
    expected.sort_by_key(|(id, ..)| *id);
    assert_eq!(observed(&beside), expected);
}

/// `replace_world` swaps a snapshot in: the old entities are gone, the
/// snapshot's are there under their saved identities.
#[test]
fn replace_world_reads_a_snapshot() {
    let service = SerializationService::new();
    let source = authored_world("Saved");
    let file = fastest(&source);

    let mut world = authored_world("Old");
    let old_ids: Vec<_> = observed(&world).into_iter().map(|(id, ..)| id).collect();
    let report = service
        .replace_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the replace failed: {e:?}"));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(observed(&world), observed(&source));
    for id in old_ids {
        assert_eq!(world.entity_with_id(id), None, "{id:?} remains");
    }
}

/// A snapshot that cannot be loaded — cut short, from another schema, or not
/// a snapshot at all — leaves the world exactly as it was, through both
/// `load_world` and `replace_world`.
#[test]
fn a_damaged_snapshot_leaves_the_world_unchanged() {
    let service = SerializationService::new();
    let file = fastest(&authored_world("Saved"));

    let fingerprint = (registration_of("Transform")
        .expect("Transform is registered")
        .schema)()
    .to_le_bytes();
    let at = file
        .payload
        .windows(fingerprint.len())
        .position(|window| window == fingerprint)
        .expect("the snapshot lists Transform's fingerprint");
    let mut other_schema = file.payload.clone();
    other_schema[at] ^= 0x01;

    let cases = [
        (
            "cut short",
            with_payload(&file, file.payload[..file.payload.len() / 2].to_vec()),
        ),
        ("another schema", with_payload(&file, other_schema)),
        ("garbage", with_payload(&file, vec![0xFF; 64])),
    ];
    for (case, damaged) in cases {
        let mut world = authored_world("Open");
        let before = observed(&world);
        assert!(
            service.load_world(&damaged, &mut world).is_err(),
            "{case}: load_world accepted it"
        );
        assert_eq!(
            observed(&world),
            before,
            "{case}: load_world changed the world"
        );
        assert!(
            service.replace_world(&damaged, &mut world).is_err(),
            "{case}: replace_world accepted it"
        );
        assert_eq!(
            observed(&world),
            before,
            "{case}: replace_world changed the world"
        );
    }
}
