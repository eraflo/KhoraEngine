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

//! Scene files through the serialization service: which encoding a goal
//! writes, what a load reports, and that a load which cannot go through
//! leaves the world as it was.

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, SceneHeader, SerializationGoal, HEADER_MAGIC_BYTES};
use khora_data::ecs::{Name, Parent, Transform, World};
use khora_data::scene::record::Record;
use khora_data::scene::{capture_world, SceneEncoding, SceneFileReadError, TextEncoding};
use khora_io::serialization::{SerializationService, SerializationServiceError};

/// A scene file of `version`, written in `encoding`.
fn scene_file(version: u8, encoding: &str, payload: Vec<u8>) -> SceneFile {
    let mut encoding_id = [0u8; 32];
    encoding_id[..encoding.len()].copy_from_slice(encoding.as_bytes());
    SceneFile {
        header: SceneHeader {
            magic_bytes: HEADER_MAGIC_BYTES,
            format_version: version,
            encoding_id,
            payload_length: payload.len() as u64,
        },
        payload,
    }
}

/// The encoding a file's header names.
fn encoding_of(file: &SceneFile) -> &str {
    std::str::from_utf8(&file.header.encoding_id)
        .expect("the encoding id is UTF-8")
        .trim_end_matches('\0')
}

/// A small authored scene: a parent and a child, with names.
fn authored_world(prefix: &str) -> World {
    let mut world = World::new();
    let root = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new(format!("{prefix} root")),
    ));
    let child = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.1, 0.0)),
        Name::new(format!("{prefix} child")),
    ));
    world.set_parent(child, Some(root));
    for entity in [root, child] {
        world.mark_authored(entity).expect("authored");
    }
    world
}

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
        .map(|e: EntityId| {
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

/// Every goal writes a version-2 file in the encoding it implies — text for
/// people and for the long term, compact for the editor and for size, a
/// snapshot for speed, MessagePack for other tools — and every one of them
/// loads back into the same scene, identities included.
#[test]
fn every_goal_writes_a_version_2_file_that_loads_back() {
    let service = SerializationService::new();
    let source = authored_world("Saved");
    let expected = observed(&source);

    for (goal, encoding) in [
        (SerializationGoal::HumanReadableDebug, "KH_TEXT_V2"),
        (SerializationGoal::LongTermStability, "KH_TEXT_V2"),
        (SerializationGoal::EditorInterchange, "KH_COMPACT_V2"),
        (SerializationGoal::SmallestFileSize, "KH_COMPACT_V2"),
        (SerializationGoal::FastestLoad, "KH_SNAPSHOT_V1"),
        (SerializationGoal::PortableBinary, "KH_MSGPACK_V2"),
    ] {
        let file = service
            .save_world(&source, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: save failed: {e:?}"));
        assert_eq!(file.header.format_version, 2, "{goal:?}");
        assert_eq!(encoding_of(&file), encoding, "{goal:?}");

        let mut loaded = World::new();
        service
            .load_world(&file, &mut loaded)
            .unwrap_or_else(|e| panic!("{goal:?}: load failed: {e:?}"));
        assert_eq!(observed(&loaded), expected, "{goal:?}");
    }
}

/// Opening a scene replaces the world with it: the old entities are gone, the
/// scene's are there under their saved identities, and the report says the
/// file read exactly as written.
#[test]
fn replace_world_swaps_in_the_scene_and_reports() {
    let service = SerializationService::new();
    let scene = authored_world("Scene");
    let file = service
        .save_world(&scene, SerializationGoal::EditorInterchange)
        .expect("saves");

    let mut world = authored_world("Old");
    let old_ids: Vec<_> = observed(&world).into_iter().map(|(id, ..)| id).collect();
    let report = service
        .replace_world(&file, &mut world)
        .unwrap_or_else(|e| panic!("the replace failed: {e:?}"));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(observed(&world), observed(&scene));
    for id in old_ids {
        assert_eq!(
            world.entity_with_id(id),
            None,
            "{id:?} from the old world remains"
        );
    }
}

/// Opening a scene that cannot be loaded — a component nobody knows, a payload
/// that is not a scene — keeps the world that was open, entirely.
#[test]
fn replace_world_is_atomic() {
    let service = SerializationService::new();

    let mut record = capture_world(&authored_world("Broken")).expect("captures");
    let child = record.entities[1];
    let page = record
        .pages
        .iter_mut()
        .find(|page| page.rows.contains(&child))
        .expect("the child's components are recorded");
    page.components.push("NoSuchComponent".into());
    page.columns.push(vec![Record::Unit; page.rows.len()]);
    let unknown = scene_file(
        2,
        "KH_TEXT_V2",
        TextEncoding.encode(&record).expect("encodes"),
    );
    let damaged = scene_file(2, "KH_COMPACT_V2", vec![0xFF; 64]);

    for (case, file) in [("unknown component", unknown), ("damaged payload", damaged)] {
        let mut world = authored_world("Open");
        let before = observed(&world);
        let outcome = service.replace_world(&file, &mut world);
        assert!(outcome.is_err(), "{case}: the replace must fail");
        assert_eq!(observed(&world), before, "{case}: the open world changed");
    }
}

/// The refusal a file of an older scene format meets, whichever way the
/// service was asked to read it.
fn old_format_refusal(error: SerializationServiceError) -> SceneFileReadError {
    match error {
        SerializationServiceError::ReadFailed(refusal) => refusal,
        other => panic!("an old file must be refused as unreadable, got {other:?}"),
    }
}

/// A file from before scene records is not guessed at: loading it fails as
/// an old format, with a message that says so and names no command — nothing
/// reads that format any more — and the world it was loaded into is
/// unchanged.
#[test]
fn a_v1_file_is_refused_as_older_than_the_engine_reads() {
    let service = SerializationService::new();
    // The version alone refuses it: the encoding the header names, and the
    // payload behind it, are never looked at.
    let old = scene_file(1, "KH_TEXT_V2", vec![0; 16]);

    let refusal = old_format_refusal(
        service
            .load_world(&old, &mut World::new())
            .expect_err("a version-1 file is refused"),
    );
    assert_eq!(refusal, SceneFileReadError::OldFormat(1));
    let message = refusal.to_string();
    assert_eq!(
        message, "scene format v1 is older than any this engine reads",
        "the refusal says why"
    );
    for command in ["cargo", "xtask", "`"] {
        assert!(
            !message.contains(command),
            "the refusal names no command (`{command}`): {message}"
        );
    }

    let mut world = authored_world("Open");
    let before = observed(&world);
    let refusal = old_format_refusal(
        service
            .replace_world(&old, &mut world)
            .expect_err("a version-1 file is refused"),
    );
    assert_eq!(refusal, SceneFileReadError::OldFormat(1));
    assert_eq!(observed(&world), before, "a refused file changes nothing");
}

/// Every version below the engine's is refused the same way, carrying the
/// version the file was written in.
#[test]
fn a_v0_file_is_refused_as_an_old_format_too() {
    let service = SerializationService::new();
    let old = scene_file(0, "KH_COMPACT_V2", vec![0xFF; 8]);

    let refusal = old_format_refusal(
        service
            .load_world(&old, &mut World::new())
            .expect_err("a version-0 file is refused"),
    );
    assert_eq!(refusal, SceneFileReadError::OldFormat(0));
    assert_eq!(
        refusal.to_string(),
        "scene format v0 is older than any this engine reads"
    );
}
