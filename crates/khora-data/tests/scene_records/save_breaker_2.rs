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

//! Game saves under attack, second round: a save older than a rename, a
//! hostile save's entity lists, a damaged base, and an entity the author
//! added beside a subtree play rearranged.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::{Light, Name, Transform};
use khora_data::scene::{capture_save, compose, prepare_game, SaveRecord};

use super::renames::{Beacon, Stamina};
use super::*;

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

fn twin(dst: &World, id: PersistentId) -> EntityId {
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

/// Renames every field `from` of every struct cell of `record` to `to`,
/// returning how many.
fn renamed_field(record: &mut SceneRecord, from: &str, to: &str) -> usize {
    let mut renamed = 0;
    for page in &mut record.pages {
        for column in &mut page.columns {
            for cell in column {
                if let Record::Struct { fields, .. } = cell {
                    for (name, _) in fields.iter_mut() {
                        if name == from {
                            *name = to.to_owned();
                            renamed += 1;
                        }
                    }
                }
            }
        }
    }
    renamed
}

/// **A save older than a type rename.** The save was written when `Beacon`
/// was called `LegacyBeacon`: both what the game made of the beacon and what
/// the scene held when the save was taken name it so. Loaded by today's
/// build, today's `Beacon` carries the range the game set.
#[test]
fn a_save_older_than_a_component_rename_loads() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), Beacon { range: 42.5 }));
    let entity_id = world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");
    world
        .get_mut::<Transform>(entity)
        .expect("placed")
        .translation = Vec3::new(0.0, 4.0, 0.0);
    world.get_mut::<Beacon>(entity).expect("there").range = 10.0;
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    assert_eq!(
        rename_component(&mut save.changes, "Beacon", "LegacyBeacon"),
        1,
        "the older build wrote the changed beacon under its former name"
    );
    assert_eq!(
        rename_component(&mut save.before, "Beacon", "LegacyBeacon"),
        1,
        "and the scene's beacon at save time too"
    );

    let dst = load(&base, &save);
    let back = twin(&dst, entity_id);
    assert_eq!(dst.get::<Beacon>(back), Some(&Beacon { range: 10.0 }));
    assert_eq!(
        dst.get::<Transform>(back).map(|t| t.translation),
        Some(Vec3::new(0.0, 4.0, 0.0))
    );
}

/// **A removal older than a type rename.** The game took the beacon off; the
/// save, written before the rename, names it by its former name. It stays off.
#[test]
fn a_removal_older_than_a_component_rename_stays_removed() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), Beacon { range: 42.5 }));
    let entity_id = world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.remove_component::<Beacon>(entity).expect("off");
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    for entry in &mut save.removed {
        for name in &mut entry.components {
            if name == "Beacon" {
                *name = "LegacyBeacon".to_owned();
            }
        }
    }

    let dst = load(&base, &save);
    assert!(
        dst.get::<Beacon>(twin(&dst, entity_id)).is_none(),
        "the beacon play removed came back"
    );
}

/// **A save older than a field rename.** The game drained the stamina; the
/// save, written when `reserve` was `points`, names it so both in what the
/// game made of it and in the scene's value at save time. The author has
/// since retuned `regen`, which the game left alone. Today's build loads the
/// drained reserve and the author's new regen.
#[test]
fn a_patch_older_than_a_field_rename_applies() {
    let mut world = World::new();
    let entity = world.spawn(Stamina {
        reserve: 7.5,
        regen: 0.25,
    });
    let entity_id = world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.get_mut::<Stamina>(entity).expect("there").reserve = 1.0;
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    assert!(
        renamed_field(&mut save.changes, "reserve", "points") > 0,
        "the changed stamina names its reserve"
    );
    assert!(
        renamed_field(&mut save.before, "reserve", "points") > 0,
        "the scene's stamina at save time names its reserve"
    );

    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    editor
        .get_mut::<Stamina>(twin(&editor, entity_id))
        .expect("there")
        .regen = 0.5;
    let edited = capture_world(&editor).expect("captures");

    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(&edited, &save))
        .unwrap_or_else(|e| panic!("a save older than a field rename did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    assert_eq!(
        dst.get::<Stamina>(twin(&dst, entity_id)),
        Some(&Stamina {
            reserve: 1.0,
            regen: 0.5
        })
    );
}

/// **An entity the author added after a subtree play moved.** In the scene,
/// `P` holds `A` (which holds `A1`); the author then appends `N` to `P`. Play
/// had moved `A1` under `R`. `N` was put after `A` — it must load after `A`.
#[test]
fn an_added_child_keeps_its_place_when_its_scene_predecessor_moved() {
    let mut world = World::new();
    let r = world.spawn(Name::new("R"));
    let p = world.spawn(Name::new("P"));
    let a = world.spawn(Name::new("A"));
    let a1 = world.spawn(Name::new("A1"));
    for entity in [r, p, a, a1] {
        world.mark_authored(entity).expect("authored");
    }
    assert!(world.set_parent(a, Some(p)));
    assert!(world.set_parent(a1, Some(a)));
    let base = capture_world(&world).expect("captures");
    let (p_id, a_id) = (id(&world, p), id(&world, a));

    // The author appends `N` to `P`.
    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    let n = editor.spawn(Name::new("N"));
    let n_id = editor.mark_authored(n).expect("authored");
    assert!(editor.set_parent(n, Some(twin(&editor, p_id))));
    let edited = capture_world(&editor).expect("captures");

    // Play moved `A1` under `R`.
    assert!(world.set_parent(a1, Some(r)));
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let dst = load(&edited, &save);
    let children: Vec<PersistentId> = dst
        .get::<Children>(twin(&dst, p_id))
        .expect("P has children")
        .0
        .iter()
        .map(|child| dst.persistent_id(*child).expect("an id"))
        .collect();
    assert_eq!(children, vec![a_id, n_id], "the author's order");
}

/// **One entity listed twice in a hostile save's order** is refused or loaded
/// once — never two entities under one identity, never a panic.
#[test]
fn a_hostile_order_listing_an_entity_twice_never_makes_two() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), Name::new("A")));
    world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");
    let made = world.spawn((Transform::identity(), Name::new("Made")));
    let made_id = id(&world, made);
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    save.order.push(made_id);
    save.order.push(id(&world, entity));

    let mut dst = World::new();
    if let Ok(prepared) = prepare_game(&mut dst, &compose(&base, &save)) {
        prepared.commit(&mut dst, Identity::Keep);
        assert_eq!(dst.iter_entities().count(), 2, "an entity was doubled");
    }
}

/// **A damaged base** is an error for a save and for a load, as it is for
/// loading the scene itself — never a panic, never a world quietly missing
/// what the damaged rows held. Here the row of an entity the game left alone
/// lost its `Name` column.
#[test]
fn a_damaged_base_is_an_error_for_a_save_and_a_load() {
    let mut world = World::new();
    let a = world.spawn((Transform::identity(), Name::new("A")));
    let b = world.spawn((Transform::identity(), Name::new("B")));
    world.mark_authored(a).expect("authored");
    let b_id = world.mark_authored(b).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.get_mut::<Transform>(a).expect("placed").translation.x = 3.0;
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let mut ragged = base.clone();
    let page = isolate(&mut ragged, b_id, "Name");
    let column = ragged.pages[page]
        .components
        .iter()
        .position(|name| name == "Name")
        .expect("named");
    ragged.pages[page].columns[column].clear();
    assert!(
        apply(&mut World::new(), &ragged, Identity::Keep).is_err(),
        "the damaged scene itself is refused"
    );

    assert!(capture_save(&world, AssetUUID::new(), &ragged).is_err());
    let mut dst = World::new();
    if let Ok(prepared) = prepare_game(&mut dst, &compose(&ragged, &save)) {
        prepared.commit(&mut dst, Identity::Keep);
        panic!(
            "a game loaded over a damaged scene; B's name is {:?}",
            dst.get::<Name>(twin(&dst, b_id))
        );
    }
}

/// **A component the author removed since the save.** The game switched
/// `A`'s light off; the author then took the light off `A` in the scene. The
/// save's patch is not a whole light: the author's removal wins, as it does
/// for an entity the author removed, and the rest of the save loads.
#[test]
fn a_patch_for_a_component_the_author_removed_since_does_not_sink_the_load() {
    let mut world = World::new();
    let a = world.spawn((Transform::identity(), Name::new("A"), Light::point()));
    let a_id = world.mark_authored(a).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.get_mut::<Light>(a).expect("lit").enabled = false;
    world.get_mut::<Transform>(a).expect("placed").translation.x = 2.0;
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    editor
        .remove_component::<Light>(twin(&editor, a_id))
        .expect("the author takes the light off");
    let edited = capture_world(&editor).expect("captures");

    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(&edited, &save))
        .unwrap_or_else(|e| panic!("an authored removal made the save unloadable: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    let back = twin(&dst, a_id);
    assert_eq!(
        dst.get::<Transform>(back).map(|t| t.translation.x),
        Some(2.0)
    );
    assert!(
        dst.get::<Light>(back).is_none(),
        "the author's removal lost to a patch read as a whole light: {:?}",
        dst.get::<Light>(back)
    );
}
