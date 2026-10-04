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

//! Game saves against a scene that moves: a base read from a format that drops struct
//! names, a hierarchy rearranged by play, a scene edited between the save and
//! the load, and damaged save bytes.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::{Light, Name, Transform};
use khora_data::scene::{capture_save, compose, prepare_game, SaveRecord};

use super::*;

/// Three authored entities, as a scene holds them.
fn three() -> (World, [EntityId; 3]) {
    let mut world = World::new();
    let a = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("A"),
        Light::point(),
    ));
    let b = world.spawn((Transform::identity(), Name::new("B"), Light::point()));
    let c = world.spawn((Transform::identity(), Name::new("C")));
    for entity in [a, b, c] {
        world.mark_authored(entity).expect("authored");
    }
    (world, [a, b, c])
}

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

fn twin(dst: &World, id: PersistentId) -> EntityId {
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// **A delta against a text scene.** Scenes are authored in text to be read
/// and diffed; a save of a world that changed nothing against such a scene
/// must be as empty as one against a compact scene.
#[test]
fn a_save_against_a_text_base_of_an_unchanged_world_is_empty() {
    let (world, _) = three();
    let base = capture_world(&world).expect("captures");
    for (name, encoding) in every_encoding() {
        let read_base = through(&base, name, encoding);
        let save = capture_save(&world, AssetUUID::new(), &read_base).expect("saves");
        assert!(
            save.changes.entities.is_empty(),
            "{name}: an unchanged world lists {} entities as changed",
            save.changes.entities.len()
        );
    }
}

/// **The designer edit, against a text scene.** The game never touched `B`;
/// the designer renames it in the scene after the save. Loading must give
/// `B` its new name, whatever encoding the scene is kept in.
#[test]
fn a_designer_edit_reaches_an_untouched_entity_with_a_text_base() {
    let (mut world, [a, b, _]) = three();
    let base = capture_world(&world).expect("captures");
    let b_id = id(&world, b);
    world.get_mut::<Transform>(a).expect("placed").translation.y = 9.0;

    for (name, encoding) in every_encoding() {
        let read_base = through(&base, name, encoding);
        let save = capture_save(&world, AssetUUID::new(), &read_base).expect("saves");

        let mut editor = World::new();
        apply(&mut editor, &read_base, Identity::Keep).expect("the scene opens");
        editor
            .get_mut::<Name>(twin(&editor, b_id))
            .expect("named")
            .0 = "B, renamed".into();
        let edited = through(&capture_world(&editor).expect("captures"), name, encoding);

        let dst = load(&edited, &save);
        assert_eq!(
            dst.get::<Name>(twin(&dst, b_id)).map(Name::as_str),
            Some("B, renamed"),
            "{name}: the designer's rename of an untouched entity was lost"
        );
    }
}

/// **A reparent keeps its place among the new siblings.** Play moves `A`
/// from under `P1` to under `P2`, after `X`: `P2`'s children are `[X, A]`,
/// and a load must give them back in that order.
#[test]
fn a_reparented_entity_keeps_its_place_among_its_new_siblings() {
    let mut world = World::new();
    let p1 = world.spawn(Name::new("P1"));
    let a = world.spawn(Name::new("A"));
    let p2 = world.spawn(Name::new("P2"));
    let x = world.spawn(Name::new("X"));
    for entity in [p1, a, p2, x] {
        world.mark_authored(entity).expect("authored");
    }
    assert!(world.set_parent(a, Some(p1)));
    assert!(world.set_parent(x, Some(p2)));
    let base = capture_world(&world).expect("captures");

    assert!(world.set_parent(a, Some(p2)));
    let expected: Vec<PersistentId> = world
        .get::<Children>(p2)
        .expect("P2 has children")
        .0
        .iter()
        .map(|child| id(&world, *child))
        .collect();
    assert_eq!(expected, vec![id(&world, x), id(&world, a)]);

    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    let dst = load(&base, &save);
    let order: Vec<PersistentId> = dst
        .get::<Children>(twin(&dst, id(&world, p2)))
        .expect("P2 has children")
        .0
        .iter()
        .map(|child| dst.persistent_id(*child).expect("an id"))
        .collect();
    assert_eq!(order, expected, "the sibling order play made was lost");
}

/// **A base entity the designer deleted since the save.** The save patches
/// `A`; the scene no longer has it. The patch is not a whole value — the
/// load must not build `A` out of it as if it were.
#[test]
fn a_patch_for_an_entity_the_scene_lost_is_not_loaded_as_a_whole_value() {
    let (mut world, [a, _, _]) = three();
    let base = capture_world(&world).expect("captures");
    let a_id = id(&world, a);
    world.get_mut::<Transform>(a).expect("placed").translation.y = 9.0;
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let mut edited = base.clone();
    without_entities(&mut edited, &[a_id]);

    let mut dst = World::new();
    match prepare_game(&mut dst, &compose(&edited, &save)) {
        Ok(prepared) => {
            let report = prepared.commit(&mut dst, Identity::Keep).report;
            if let Some(entity) = dst.entity_with_id(a_id) {
                let transform = dst.get::<Transform>(entity).copied();
                panic!(
                    "a patch became an entity: {transform:?}, light {:?}, report {report:?}",
                    dst.get::<Light>(entity)
                );
            }
        }
        Err(error) => panic!("a scene edit made the save unloadable: {error}"),
    }
}

/// **A damaged compact save prelude** is an error, never a panic or an
/// allocation the input does not back.
#[test]
fn a_damaged_compact_save_prelude_is_an_error() {
    let mut huge = vec![0u8; 16];
    huge.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]);
    for (what, bytes) in [
        ("empty", Vec::new()),
        ("a truncated base", vec![1, 2, 3]),
        ("a huge destroyed count", huge),
    ] {
        assert!(
            CompactEncoding.decode_save(&bytes).is_err(),
            "{what}: decoded"
        );
    }
}

/// **Two runs on one base.** A game loaded from a save, played on, and saved
/// again against the same scene: the first run's spawn keeps its identity,
/// and the second run's spawn gets one of its own.
#[test]
fn a_second_save_against_the_same_base_keeps_every_identity_apart() {
    let (mut world, _) = three();
    let base = capture_world(&world).expect("captures");
    let first = world.spawn((Transform::identity(), Name::new("First")));
    let first_id = id(&world, first);
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let mut again = load(&base, &save);
    let second = again.spawn((Transform::identity(), Name::new("Second")));
    let second_id = id(&again, second);
    assert_ne!(second_id, first_id, "a new spawn took a loaded identity");

    let resave = capture_save(&again, AssetUUID::new(), &base).expect("saves");
    let dst = load(&base, &resave);
    assert_eq!(
        dst.get::<Name>(twin(&dst, first_id)).map(Name::as_str),
        Some("First")
    );
    assert_eq!(
        dst.get::<Name>(twin(&dst, second_id)).map(Name::as_str),
        Some("Second")
    );
    assert_eq!(dst.iter_entities().count(), 5);
}

/// **An authored entity stripped bare.** Every component it had removed, it
/// still exists, with nothing on it.
#[test]
fn an_authored_entity_stripped_of_every_component_loads_bare() {
    let (mut world, [_, _, c]) = three();
    let base = capture_world(&world).expect("captures");
    let c_id = id(&world, c);
    world.remove_component::<Transform>(c).expect("off");
    world.remove_component::<Name>(c).expect("off");
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    assert!(save.changes.entities.contains(&c_id));

    let dst = load(&base, &save);
    let back = twin(&dst, c_id);
    assert!(dst.get::<Name>(back).is_none());
    assert!(dst.get::<Transform>(back).is_none());
}
