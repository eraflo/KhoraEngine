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

//! Game saves: what a running world changed against the scene it started
//! from, and the world brought back from the two.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::{FrozenValue, ScriptSnapshot, ScriptValue};
use khora_data::ecs::{BodyMotion, Light, Name, Script, ScriptState, Tag, Transform};
use khora_data::scene::{capture_save, compose, prepare_game, SaveRecord};

use super::sample::suspended_snapshot;
use super::*;

/// The scene a game starts from: three authored entities.
struct Scene {
    world: World,
    /// A guard: placed, named, lit.
    a: EntityId,
    /// A bystander: placed, named, lit.
    b: EntityId,
    /// A scripted sentry.
    c: EntityId,
}

fn scene() -> Scene {
    let mut world = World::new();
    let a = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("A"),
        Light::point(),
    ));
    let b = world.spawn((
        Transform::from_translation(Vec3::new(-1.0, 0.0, 0.0)),
        Name::new("B"),
        Light::point(),
    ));
    let c = world.spawn((
        Transform::identity(),
        Name::new("C"),
        Script::new("ai/sentry.erg", "Sentry").with_field("speed", ScriptValue::Float(2.0)),
    ));
    for entity in [a, b, c] {
        world.mark_authored(entity).expect("authored");
    }
    Scene { world, a, b, c }
}

fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

/// `base` and `save` composed and brought into a fresh world.
fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

/// The entity `dst` knows by `id`.
fn twin(dst: &World, id: PersistentId) -> EntityId {
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// Every cell `record` holds for `entity`, by component name, across pages.
fn cells_of(record: &SceneRecord, entity: PersistentId) -> Vec<(String, Record)> {
    let mut cells = Vec::new();
    for page in &record.pages {
        if let Some(row) = page.rows.iter().position(|known| *known == entity) {
            for (name, column) in page.components.iter().zip(&page.columns) {
                cells.push((name.clone(), column[row].clone()));
            }
        }
    }
    cells
}

fn holds_row(record: &SceneRecord, entity: PersistentId) -> bool {
    record.pages.iter().any(|page| page.rows.contains(&entity))
}

/// Nothing played, nothing saved: a world exactly as its scene has no
/// change, no destruction, and names its base.
#[test]
fn a_save_of_an_unchanged_world_is_empty() {
    let scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let base_id = AssetUUID::new();

    let save = capture_save(&scene.world, base_id, &base).expect("saves");

    assert_eq!(save.base, base_id);
    assert!(save.destroyed.is_empty(), "{:?}", save.destroyed);
    assert!(save.changes.entities.is_empty(), "{:?}", save.changes);
    assert!(save.changes.pages.is_empty(), "{:?}", save.changes);
}

/// **A save holds what the game changed, and only that** — by entity and by
/// component. An authored entity left alone has no row. One moved up has a
/// row holding its `Transform` alone, whole, as it is now, and the save keeps
/// beside it the scene's `Transform` at save time — what a load compares the
/// two with — and nothing else: no cell for a component the game left alone.
#[test]
fn a_save_is_a_delta() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let (a, b) = (id(&scene.world, scene.a), id(&scene.world, scene.b));

    scene
        .world
        .get_mut::<Transform>(scene.a)
        .expect("placed")
        .translation
        .y = 9.0;
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    assert!(
        !holds_row(&save.changes, b),
        "an unchanged entity has no row"
    );
    assert!(!save.changes.entities.contains(&b));
    assert!(save.changes.entities.contains(&a));

    let now = capture_world(&scene.world).expect("captures");
    let transform_of = |record: &SceneRecord| {
        cells_of(record, a)
            .into_iter()
            .find(|(name, _)| name == "Transform")
            .map(|(_, cell)| cell)
            .expect("the entity has a Transform")
    };
    assert_eq!(
        cells_of(&save.changes, a),
        vec![("Transform".to_owned(), transform_of(&now))],
        "the row holds the changed component alone, whole, as it is now"
    );
    assert_eq!(
        cells_of(&save.before, a),
        vec![("Transform".to_owned(), transform_of(&base))],
        "the scene's value of the changed component, at save time"
    );
    assert!(
        save.before
            .pages
            .iter()
            .all(|page| page.rows.iter().all(|row| *row == a)),
        "the save keeps no scene value of anything else: {:?}",
        save.before
    );
    assert!(!holds_row(&save.before, b));
}

/// **The point of saving a delta.** A designer retunes the scene after the
/// save was taken; loading the save, every value the game did not change
/// takes the new authored one — on entities the game never touched, and on
/// the untouched fields of one it did.
#[test]
fn a_designer_edit_reaches_entities_that_did_not_diverge() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let (a, b) = (id(&scene.world, scene.a), id(&scene.world, scene.b));

    scene
        .world
        .get_mut::<Transform>(scene.a)
        .expect("placed")
        .translation
        .y = 9.0;
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    // The designer opens the scene, edits it and saves it again.
    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    let edited_a = twin(&editor, a);
    let edited_b = twin(&editor, b);
    editor.get_mut::<Transform>(edited_a).expect("placed").scale = Vec3::new(2.0, 2.0, 2.0);
    editor.get_mut::<Light>(edited_a).expect("lit").enabled = false;
    editor.get_mut::<Light>(edited_b).expect("lit").enabled = false;
    editor.get_mut::<Name>(edited_b).expect("named").0 = "B, renamed".into();
    let edited = capture_world(&editor).expect("captures");

    let dst = load(&edited, &save);

    let a_back = twin(&dst, a);
    let transform = dst.get::<Transform>(a_back).expect("placed");
    assert_eq!(transform.translation.y, 9.0, "what the game changed");
    assert_eq!(transform.scale, Vec3::new(2.0, 2.0, 2.0), "the edit");
    assert_eq!(dst.get::<Light>(a_back).map(|l| l.enabled), Some(false));

    let b_back = twin(&dst, b);
    assert_eq!(dst.get::<Light>(b_back).map(|l| l.enabled), Some(false));
    assert_eq!(
        dst.get::<Name>(b_back).map(Name::as_str),
        Some("B, renamed")
    );
}

/// **A tombstone.** An authored entity the game destroyed is listed as
/// destroyed, holds no row, and does not come back on load.
#[test]
fn a_destroyed_authored_entity_stays_destroyed() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let b = id(&scene.world, scene.b);

    scene.world.despawn(scene.b);
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    assert_eq!(save.destroyed, vec![b]);
    assert!(!save.changes.entities.contains(&b));
    assert!(!holds_row(&save.changes, b));

    let dst = load(&base, &save);
    assert!(dst.entity_with_id(b).is_none(), "the bystander stays dead");
    assert_eq!(dst.iter_entities().count(), 2, "the other two are there");
}

/// **A runtime spawn.** An entity the game created is not in the scene, so
/// the save holds it whole — every cell its full value — and loads it back
/// under its created identity.
#[test]
fn a_created_entity_is_restored_in_full() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");

    let spawned = scene.world.spawn((
        Transform::from_translation(Vec3::new(9.0, 9.0, 9.0)),
        Name::new("Spawned"),
        Light::directional(),
    ));
    let spawned_id = id(&scene.world, spawned);
    assert!(spawned_id.is_created(), "the game spawned it");

    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");
    let whole = capture_world(&scene.world).expect("captures");

    assert!(save.changes.entities.contains(&spawned_id));
    let mut cells = cells_of(&save.changes, spawned_id);
    let mut expected = cells_of(&whole, spawned_id);
    cells.sort_by(|x, y| x.0.cmp(&y.0));
    expected.sort_by(|x, y| x.0.cmp(&y.0));
    assert_eq!(cells, expected, "every value whole");

    let dst = load(&base, &save);
    let back = twin(&dst, spawned_id);
    assert_eq!(dst.persistent_id(back), Some(spawned_id));
    assert_eq!(
        components_of(&dst, back),
        components_of(&scene.world, spawned)
    );
    assert_eq!(dst.iter_entities().count(), 4);
}

/// A component the game removed from an authored entity stays removed.
#[test]
fn a_removed_component_stays_removed() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let a = id(&scene.world, scene.a);

    scene
        .world
        .remove_component::<Light>(scene.a)
        .expect("the light comes off");
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    assert!(
        cells_of(&save.changes, a)
            .iter()
            .all(|(name, _)| name != "Light"),
        "the row's signature has no Light"
    );
    let dst = load(&base, &save);
    let back = twin(&dst, a);
    assert!(dst.get::<Light>(back).is_none(), "the light stays off");
    assert_eq!(dst.get::<Name>(back).map(Name::as_str), Some("A"));
    assert_eq!(
        dst.get::<Transform>(back).map(|t| t.translation),
        Some(Vec3::new(1.0, 2.0, 3.0))
    );
}

/// A component the game added to an authored entity is restored, whole.
#[test]
fn an_added_component_is_restored() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let a = id(&scene.world, scene.a);

    let mut tags = Tag::new();
    tags.insert("alerted");
    scene
        .world
        .add_component(scene.a, tags.clone())
        .expect("a tag attaches");
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    let whole = capture_world(&scene.world).expect("captures");
    let tag_cell = |record: &SceneRecord| {
        cells_of(record, a)
            .into_iter()
            .find(|(name, _)| name == "Tag")
            .map(|(_, cell)| cell)
    };
    assert_eq!(tag_cell(&save.changes), tag_cell(&whole), "the whole value");

    let dst = load(&base, &save);
    assert_eq!(dst.get::<Tag>(twin(&dst, a)), Some(&tags));
}

/// **What a save resumes from.** A runtime component declared resumable —
/// `ScriptState` — travels in the save; one that is not — `BodyMotion` — is
/// not, and the load does not bring it back.
#[test]
fn a_resumable_runtime_component_is_kept_and_a_non_resumable_one_is_not() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let c = id(&scene.world, scene.c);

    let state = ScriptState {
        behavior: "Sentry".into(),
        snapshot: ScriptSnapshot {
            state: Some("Alert".into()),
            ..ScriptSnapshot::default()
        }
        .with_field("speed", ScriptValue::Float(4.0)),
    };
    scene
        .world
        .add_component(scene.c, state.clone())
        .expect("the state attaches");
    scene
        .world
        .add_component(
            scene.c,
            BodyMotion {
                linear: Vec3::new(1.0, 0.0, 0.0),
                angular: Vec3::ZERO,
            },
        )
        .expect("the motion attaches");

    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");
    let cells = cells_of(&save.changes, c);
    assert!(
        cells.iter().any(|(name, _)| name == "ScriptState"),
        "{cells:?}"
    );
    assert!(
        cells.iter().all(|(name, _)| name != "BodyMotion"),
        "{cells:?}"
    );

    let dst = load(&base, &save);
    let back = twin(&dst, c);
    assert_eq!(dst.get::<ScriptState>(back), Some(&state));
    assert!(dst.get::<BodyMotion>(back).is_none());
    assert_eq!(
        dst.get::<Script>(back),
        scene.world.get::<Script>(scene.c),
        "the authored script, unchanged"
    );
}

/// An entity inside resumed state — a register of a sequence frozen at an
/// `await` — names the loaded entity, whether the scene authored it or the
/// game created it.
#[test]
fn a_reference_in_resumed_state_is_remapped() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let (a, c) = (id(&scene.world, scene.a), id(&scene.world, scene.c));

    let decoy = scene.world.spawn(Transform::identity());
    scene.world.despawn(decoy);
    let spawned = scene
        .world
        .spawn((Transform::identity(), Name::new("Spawned")));
    let spawned_id = id(&scene.world, spawned);

    let mut snapshot = suspended_snapshot(scene.a);
    if let Some(pending) = snapshot.pending.as_mut() {
        pending.machine.registers.push(FrozenValue::Entity(spawned));
    }
    scene
        .world
        .add_component(
            scene.c,
            ScriptState {
                behavior: "Sentry".into(),
                snapshot,
            },
        )
        .expect("the state attaches");

    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");
    let dst = load(&base, &save);

    let state = dst
        .get::<ScriptState>(twin(&dst, c))
        .expect("the state loaded");
    let registers = &state
        .snapshot
        .pending
        .as_ref()
        .expect("the pending sequence")
        .machine
        .registers;
    assert!(
        registers.contains(&FrozenValue::Entity(twin(&dst, a))),
        "the authored target: {registers:?}"
    );
    assert!(
        registers.contains(&FrozenValue::Entity(twin(&dst, spawned_id))),
        "the spawned target: {registers:?}"
    );
}

/// **All of it or none of it.** A save holding a value that cannot be read —
/// or a component nobody registered — is refused before the world is touched.
#[test]
fn a_game_load_is_atomic() {
    let mut scene = scene();
    let base = capture_world(&scene.world).expect("captures");
    let c = id(&scene.world, scene.c);
    scene
        .world
        .get_mut::<Transform>(scene.a)
        .expect("placed")
        .translation
        .x = 5.0;
    scene
        .world
        .add_component(
            scene.c,
            ScriptState {
                behavior: "Sentry".into(),
                snapshot: ScriptSnapshot::default(),
            },
        )
        .expect("the state attaches");
    let save = capture_save(&scene.world, AssetUUID::new(), &base).expect("saves");

    let mut damaged = save.clone();
    *value_mut(&mut damaged.changes, c, "ScriptState") = Record::Str("garbage".into());
    let mut unknown = save.clone();
    let page = slot_of(&unknown.changes, c, "ScriptState")
        .map(|(page, _, _)| page)
        .expect("the state is recorded");
    add_column(&mut unknown.changes, page, "NoSuchComponent", Record::Unit);

    for (what, bad) in [
        ("a damaged value", damaged),
        ("an unknown component", unknown),
    ] {
        let mut dst = World::new();
        let resident = dst.spawn((Transform::identity(), Name::new("Resident")));
        let resident_id = dst.mark_authored(resident).expect("authored");
        let before = components_of(&dst, resident);

        match prepare_game(&mut dst, &compose(&base, &bad)) {
            Ok(prepared) => {
                prepared.abandon(&mut dst);
                panic!("{what}: the load was not refused");
            }
            Err(error) => assert!(!error.message.is_empty(), "{what}"),
        }
        assert_eq!(dst.iter_entities().count(), 1, "{what}: entities");
        assert_eq!(dst.persistent_id(resident), Some(resident_id), "{what}");
        assert_eq!(components_of(&dst, resident), before, "{what}");
        assert!(dst.get::<ScriptState>(resident).is_none(), "{what}");
    }
}
