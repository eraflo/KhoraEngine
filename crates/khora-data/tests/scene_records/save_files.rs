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

//! Game saves as files, and a save promoted into the scene it was taken
//! against.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::{SceneFile, HEADER_MAGIC_BYTES, SAVE_MAGIC_BYTES};
use khora_core::script::ScriptSnapshot;
use khora_data::ecs::{Light, Name, Script, ScriptState, Tag, Transform};
use khora_data::scene::{
    capture_save, compose, prepare_game, promote, read_save_file, read_scene_file, write_save_file,
    write_scene_file, SaveRecord, SceneFileReadError,
};

use super::sample::sample_world;
use super::*;

/// The sample world as a scene, then played: one entity moved, one
/// destroyed, one spawned, and a behavior caught mid-sequence. The world as
/// played, the base scene, and the save.
fn played_sample() -> (World, SceneRecord, SaveRecord) {
    let (mut world, sample) = sample_world();
    for entity in [
        sample.root,
        sample.child,
        sample.grandchild,
        sample.body,
        sample.guard,
        sample.panel,
    ] {
        world.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&world).expect("the base captures");

    world
        .get_mut::<Transform>(sample.root)
        .expect("placed")
        .translation = Vec3::new(4.0, 5.0, 6.0);
    world.despawn(sample.panel);
    let spawned = world.spawn((
        Transform::from_translation(Vec3::new(0.5, 0.0, 0.0)),
        Name::new("Spawned"),
    ));
    world
        .add_component(spawned, Light::point())
        .expect("a light attaches");

    let save = capture_save(&world, AssetUUID::new_v5("scenes/sample.kscene"), &base)
        .expect("the save captures");
    (world, base, save)
}

/// **Every encoding carries a save.** Its base, its tombstones, its rows and
/// cells — patches, whole values and the observed state frozen mid-sequence
/// with an entity in a register — come back, and the world they compose with
/// the base is the world that was saved.
#[test]
fn every_encoding_round_trips_a_save_record() {
    let (played, base, save) = played_sample();
    assert!(!save.destroyed.is_empty(), "the sample destroys an entity");
    assert!(
        save.changes
            .pages
            .iter()
            .any(|page| page.components.iter().any(|name| name == "ScriptState")),
        "the sample's observed state is in the save"
    );

    for (name, encoding) in every_encoding() {
        let file = write_save_file(&save, encoding)
            .unwrap_or_else(|e| panic!("{name}: the save does not encode: {e}"));
        assert_eq!(file.header.magic_bytes, SAVE_MAGIC_BYTES, "{name}");
        let file = SceneFile::from_bytes(&file.to_bytes())
            .unwrap_or_else(|e| panic!("{name}: the save file does not parse: {e:?}"));
        let back = read_save_file(&file)
            .unwrap_or_else(|e| panic!("{name}: the save does not decode: {e:?}"));

        assert_eq!(back.base, save.base, "{name}: the base");
        assert_eq!(back.destroyed, save.destroyed, "{name}: the tombstones");
        assert_eq!(
            back.changes.entities, save.changes.entities,
            "{name}: the entities"
        );
        assert_eq!(
            back.changes.pages.len(),
            save.changes.pages.len(),
            "{name}: the pages"
        );
        for (page, expected) in back.changes.pages.iter().zip(&save.changes.pages) {
            assert_eq!(page.components, expected.components, "{name}: a signature");
            assert_eq!(page.rows, expected.rows, "{name}: a page's rows");
        }
        if name == "compact" {
            assert_eq!(back, save, "compact keeps every record exactly");
        }

        let mut dst = World::new();
        let prepared = prepare_game(&mut dst, &compose(&base, &back))
            .unwrap_or_else(|e| panic!("{name}: the game does not load: {e}"));
        let applied = prepared.commit(&mut dst, Identity::Keep);
        assert!(
            applied.report.is_clean(),
            "{name}: a same-build round trip adapted something: {:?}",
            applied.report
        );
        assert_same_world(&played, &dst, name);
        let map = entity_map(&played, &dst);
        for (&entity, &loaded) in &map {
            let expected = played
                .get::<ScriptState>(entity)
                .map(|state| remap(&serde_json::to_value(state).expect("JSON"), &map));
            let got = dst
                .get::<ScriptState>(loaded)
                .map(|state| serde_json::to_value(state).expect("JSON"));
            assert_eq!(got, expected, "{name}: the observed state of {entity:?}");
        }
    }
}

/// **A save is not a scene.** Each file says what it is in its header, and a
/// reader refuses the other kind rather than misread it: opening a save as a
/// level, or loading a level as a save, is an error naming the mistake.
#[test]
fn a_save_file_is_refused_as_a_scene_and_a_scene_as_a_save() {
    let (_, base, save) = played_sample();
    for (name, encoding) in every_encoding() {
        let save_file = write_save_file(&save, encoding).expect("the save encodes");
        let scene_file = write_scene_file(&base, encoding).expect("the scene encodes");
        assert_eq!(save_file.header.magic_bytes, SAVE_MAGIC_BYTES, "{name}");
        assert_eq!(scene_file.header.magic_bytes, HEADER_MAGIC_BYTES, "{name}");

        assert_eq!(
            read_scene_file(&save_file),
            Err(SceneFileReadError::NotAScene),
            "{name}: a save read as a scene"
        );
        assert_eq!(
            read_save_file(&scene_file),
            Err(SceneFileReadError::NotASave),
            "{name}: a scene read as a save"
        );
    }
}

/// **Promote what was observed into what is authored.** Applied to the scene,
/// a save's patches and its component adds and removes reach the entities the
/// scene had — and nothing else: the game's spawns and kills are not
/// authoring, and observed state never becomes part of a scene.
#[test]
fn promote_patches_surviving_base_entities_only() {
    let mut world = World::new();
    let a = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Name::new("A"),
        Light::point(),
    ));
    let b = world.spawn((Transform::identity(), Name::new("B"), Light::point()));
    let c = world.spawn((Transform::identity(), Name::new("C")));
    let d = world.spawn((
        Transform::identity(),
        Name::new("D"),
        Script::new("ai/sentry.erg", "Sentry"),
    ));
    for entity in [a, b, c, d] {
        world.mark_authored(entity).expect("authored");
    }
    let ids: Vec<PersistentId> = [a, b, c, d]
        .iter()
        .map(|entity| world.persistent_id(*entity).expect("an id"))
        .collect();
    let base = capture_world(&world).expect("captures");

    // Played: A moved and tagged, B lost its light, C destroyed, something
    // spawned, D observed.
    world.get_mut::<Transform>(a).expect("placed").translation = Vec3::new(1.0, 3.0, 0.0);
    let mut tags = Tag::new();
    tags.insert("veteran");
    world.add_component(a, tags.clone()).expect("tagged");
    world.remove_component::<Light>(b).expect("unlit");
    world.despawn(c);
    let spawned = world.spawn((Transform::identity(), Name::new("Spawned")));
    let spawned_id = world.persistent_id(spawned).expect("an id");
    world
        .add_component(
            d,
            ScriptState {
                behavior: "Sentry".into(),
                snapshot: ScriptSnapshot::default(),
            },
        )
        .expect("observed");
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let promoted = promote(&base, &save);

    assert_eq!(
        promoted.entities, base.entities,
        "the scene's entities, all"
    );
    assert!(!promoted.entities.contains(&spawned_id), "no spawn");
    assert!(
        promoted
            .pages
            .iter()
            .all(|page| page.components.iter().all(|name| name != "ScriptState")),
        "no observed state"
    );

    let mut scene = World::new();
    apply(&mut scene, &promoted, Identity::Keep).expect("the promoted scene loads");
    let at = |n: usize| {
        scene
            .entity_with_id(ids[n])
            .unwrap_or_else(|| panic!("entity {n} is in the promoted scene"))
    };
    assert_eq!(
        scene.get::<Transform>(at(0)).map(|t| t.translation),
        Some(Vec3::new(1.0, 3.0, 0.0)),
        "A's move is authored now"
    );
    assert_eq!(scene.get::<Tag>(at(0)), Some(&tags), "and its tag");
    assert!(scene.get::<Light>(at(1)).is_none(), "B's light is removed");
    assert_eq!(scene.get::<Name>(at(1)).map(Name::as_str), Some("B"));
    assert_eq!(
        scene.get::<Name>(at(2)).map(Name::as_str),
        Some("C"),
        "C was destroyed in a run, not by an author: it stays"
    );
    assert!(scene.get::<ScriptState>(at(3)).is_none());
    assert_eq!(
        scene.get::<Script>(at(3)),
        Some(&Script::new("ai/sentry.erg", "Sentry"))
    );
    assert_eq!(scene.iter_entities().count(), 4);
}
