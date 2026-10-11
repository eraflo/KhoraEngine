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

//! Declared components in a game save: a field the game changed on a scene
//! entity, a component the game added to one, one on an entity the game
//! made — each kept by the save, brought back into a world that never
//! declared them, and promoted into the scene where the scene's own
//! entities are concerned.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::Transform;
use khora_data::scene::{
    capture_save, compose, prepare_game, promote, read_save_file, write_save_file, SaveRecord,
};

use super::declared_components::{attach, declare_both, field_of, fields_of, set_field};
use super::*;

/// A scene played: the base scene as captured, the world as played, and the
/// identities the tests follow.
///
/// The scene: `guard` holds a `Health` (`current` 80), `idle` holds one left
/// alone, `crate_` holds none. Both components are declared from the start,
/// as the project's scripts declare them, but only `Health` is in the scene.
///
/// The run: `guard`'s `Health.current` falls to 40 and it moves; `crate_` is
/// given a `Mood`; `spawned` is made, holding a `Health` that targets
/// `guard`.
struct Played {
    base: SceneRecord,
    world: World,
    guard: PersistentId,
    idle: PersistentId,
    crate_: PersistentId,
    spawned: PersistentId,
}

fn played() -> Played {
    let mut world = World::new();
    declare_both(&mut world);
    let guard = world.spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
    let idle = world.spawn(Transform::identity());
    let crate_ = world.spawn(Transform::from_translation(Vec3::new(0.0, 0.0, 4.0)));
    attach(
        &mut world,
        guard,
        "Health",
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(80)),
            ("label".into(), ScriptValue::Str("guard".into())),
            ("target".into(), ScriptValue::Entity(idle)),
        ]),
    );
    attach(
        &mut world,
        idle,
        "Health",
        ScriptValue::Struct(vec![("target".into(), ScriptValue::Entity(guard))]),
    );
    for entity in [guard, idle, crate_] {
        world.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&world).expect("the scene captures");

    set_field(&mut world, guard, "Health", "current", ScriptValue::Int(40));
    world
        .get_mut::<Transform>(guard)
        .expect("placed")
        .translation
        .y = 5.0;
    attach(
        &mut world,
        crate_,
        "Mood",
        ScriptValue::Struct(vec![
            ("n".into(), ScriptValue::Int(11)),
            (
                "tags".into(),
                ScriptValue::Array(vec![ScriptValue::Str("opened".into())]),
            ),
        ]),
    );
    let spawned = world.spawn(Transform::from_translation(Vec3::new(9.0, 9.0, 9.0)));
    attach(
        &mut world,
        spawned,
        "Health",
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(9)),
            ("target".into(), ScriptValue::Entity(guard)),
        ]),
    );

    let id = |entity| world.persistent_id(entity).expect("an id");
    Played {
        guard: id(guard),
        idle: id(idle),
        crate_: id(crate_),
        spawned: id(spawned),
        base,
        world,
    }
}

/// `base` and `save` composed and brought into a world that declares
/// nothing.
fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

fn twin(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

fn holds_row(record: &SceneRecord, entity: PersistentId) -> bool {
    record.pages.iter().any(|page| page.rows.contains(&entity))
}

/// Asserts `world` holds what the run left: the changed field, the added
/// component, the made entity — and the Rust component beside them.
fn assert_played(world: &World, game: &Played, case: &str) {
    let guard = twin(world, game.guard);
    let idle = twin(world, game.idle);
    let crate_ = twin(world, game.crate_);
    let spawned = twin(world, game.spawned);

    assert_eq!(
        field_of(world, guard, "Health", "current"),
        Some(ScriptValue::Int(40)),
        "{case}: the field the game changed"
    );
    assert_eq!(
        field_of(world, guard, "Health", "label"),
        Some(ScriptValue::Str("guard".into())),
        "{case}: a field the game left alone"
    );
    assert_eq!(
        field_of(world, guard, "Health", "target"),
        Some(ScriptValue::Entity(idle)),
        "{case}"
    );
    assert_eq!(
        world.get::<Transform>(guard).map(|t| t.translation.y),
        Some(5.0),
        "{case}: the Rust component beside it"
    );
    assert_eq!(
        field_of(world, idle, "Health", "current"),
        Some(ScriptValue::Int(100)),
        "{case}: a component the game left alone"
    );
    assert_eq!(
        field_of(world, crate_, "Mood", "n"),
        Some(ScriptValue::Int(11)),
        "{case}: the component the game added"
    );
    assert_eq!(
        field_of(world, crate_, "Mood", "tags"),
        Some(ScriptValue::Array(vec![ScriptValue::Str("opened".into())])),
        "{case}"
    );
    assert_eq!(
        field_of(world, spawned, "Health", "current"),
        Some(ScriptValue::Int(9)),
        "{case}: the entity the game made"
    );
    assert_eq!(
        field_of(world, spawned, "Health", "target"),
        Some(ScriptValue::Entity(guard)),
        "{case}: its reference to a scene entity"
    );
}

/// A field the game changed in a declared component of a scene entity is
/// in the save, and only where the game changed something: an entity whose
/// declared component the game left alone has no row.
#[test]
fn a_game_save_keeps_a_changed_declared_field() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        !holds_row(&save.changes, game.idle),
        "an entity the game left alone has no row"
    );
    let dst = load(&game.base, &save);
    let guard = twin(&dst, game.guard);
    assert_eq!(
        field_of(&dst, guard, "Health", "current"),
        Some(ScriptValue::Int(40))
    );
    assert_eq!(
        field_of(&dst, guard, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
}

/// A declared component the game added to a scene entity — one the scene
/// itself never held — is in the save and back on load.
#[test]
fn a_game_save_keeps_a_declared_component_added_at_run_time() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    let dst = load(&game.base, &save);
    let crate_ = twin(&dst, game.crate_);
    assert_eq!(
        field_of(&dst, crate_, "Mood", "n"),
        Some(ScriptValue::Int(11))
    );
    assert_eq!(
        field_of(&dst, crate_, "Mood", "stash"),
        Some(ScriptValue::Struct(vec![
            ("item".into(), ScriptValue::Str(String::new())),
            ("value".into(), ScriptValue::Int(0)),
        ])),
        "a field the game never wrote holds its declared default"
    );
}

/// A declared component on an entity the game made is in the save and back
/// on load, its reference to a scene entity remapped.
#[test]
fn a_game_save_keeps_a_declared_component_on_an_entity_made_at_run_time() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(save.created.contains(&game.spawned));
    let dst = load(&game.base, &save);
    let spawned = twin(&dst, game.spawned);
    assert_eq!(
        field_of(&dst, spawned, "Health", "current"),
        Some(ScriptValue::Int(9))
    );
    assert_eq!(
        field_of(&dst, spawned, "Health", "target"),
        Some(ScriptValue::Entity(twin(&dst, game.guard)))
    );
}

/// The scene file and the save file, each written in every encoding and
/// read back, bring the whole run back into a world that declares nothing.
#[test]
fn a_game_save_file_restores_declared_components_in_a_fresh_world() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    for (name, encoding) in every_encoding() {
        let base = through(&game.base, name, encoding);
        let file = write_save_file(&save, encoding)
            .unwrap_or_else(|e| panic!("{name}: the save encodes: {e}"));
        let file = khora_core::scene::SceneFile::from_bytes(&file.to_bytes())
            .unwrap_or_else(|e| panic!("{name}: the save file parses: {e:?}"));
        let back = read_save_file(&file).unwrap_or_else(|e| panic!("{name}: the save reads: {e}"));
        let dst = load(&base, &back);
        assert_played(&dst, &game, name);
    }
}

/// Promoting a save into its scene keeps what the game did to the scene's
/// own entities' declared components — the changed field, the added
/// component — and, as for any component, not the entities it made.
#[test]
fn promoting_a_game_save_keeps_declared_components() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    let promoted = promote(&game.base, &save);
    let mut dst = World::new();
    apply(&mut dst, &promoted, Identity::Keep)
        .unwrap_or_else(|e| panic!("the promoted scene does not load: {e}"));

    let guard = twin(&dst, game.guard);
    assert_eq!(
        field_of(&dst, guard, "Health", "current"),
        Some(ScriptValue::Int(40))
    );
    let crate_ = twin(&dst, game.crate_);
    assert_eq!(
        field_of(&dst, crate_, "Mood", "n"),
        Some(ScriptValue::Int(11))
    );
    assert!(fields_of(&dst, twin(&dst, game.idle), "Health").is_some());
    assert!(dst.entity_with_id(game.spawned).is_none());
}

/// A game save stays loadable when its scene was saved again since under
/// another declaration of a component the save holds: the scene re-saved
/// once `Health.current` became a `float` and the author tuned an entity's
/// to 99.5, the save taken while it was an `int`. Each cell is read as the record that wrote
/// it declared it: the game loads, the entity the game left alone keeps the
/// scene's value, the one it changed keeps the game's.
#[test]
fn a_game_save_loads_against_its_scene_saved_again_under_a_new_declaration() {
    use khora_data::ecs::{FieldKind, SemanticDomain};
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");

    // The scene, opened under the new declaration and saved again.
    let mut fields = super::declared_components::health_fields();
    fields[0] =
        super::declared_components::field("current", FieldKind::Float, ScriptValue::Float(100.0));
    let mut resaved = World::new();
    super::declared_components::declare(&mut resaved, "Health", SemanticDomain::Physics, fields);
    super::declared_components::declare(
        &mut resaved,
        "Mood",
        SemanticDomain::Script,
        super::declared_components::mood_fields(),
    );
    apply(&mut resaved, &game.base, Identity::Keep).expect("the scene opens");
    // The author, now that it is a float, tunes the idle entity's.
    let idle = twin(&resaved, game.idle);
    set_field(
        &mut resaved,
        idle,
        "Health",
        "current",
        ScriptValue::Float(99.5),
    );
    let base = capture_world(&resaved).expect("the scene captures");

    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(&base, &save)).unwrap_or_else(|e| {
        panic!("a save against its scene saved under a new declaration does not load: {e}")
    });
    prepared.commit(&mut dst, Identity::Keep);
    assert_eq!(
        field_of(&dst, twin(&dst, game.idle), "Health", "current").and_then(|v| match v {
            ScriptValue::Float(f) => Some(f),
            ScriptValue::Int(i) => Some(i as f32),
            _ => None,
        }),
        Some(99.5),
        "the entity the game left alone keeps the scene's value"
    );
    assert!(
        field_of(&dst, twin(&dst, game.guard), "Health", "label")
            == Some(ScriptValue::Str("guard".into())),
        "the entity the game changed keeps its `Health`"
    );
}

/// A declared component the game removed from a scene entity is gone once
/// the save is loaded — the scene's value does not come back — and stays
/// gone once the save is promoted into the scene.
#[test]
fn a_game_save_keeps_a_declared_component_removed_at_run_time() {
    let mut game = played();
    let idle = game
        .world
        .entity_with_id(game.idle)
        .expect("the idle entity is there");
    let key = game
        .world
        .components()
        .key_named("Health")
        .expect("`Health` is registered");
    assert!(
        game.world.remove_component_by_key(idle, key),
        "`Health` comes off"
    );
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        save.removed
            .iter()
            .any(|removed| removed.entity == game.idle
                && removed.components.iter().any(|name| name == "Health")),
        "the save names the removal: {:?}",
        save.removed
    );
    let dst = load(&game.base, &save);
    assert_eq!(
        fields_of(&dst, twin(&dst, game.idle), "Health"),
        None,
        "the removed component stays removed"
    );
    let promoted = promote(&game.base, &save);
    let mut scene = World::new();
    apply(&mut scene, &promoted, Identity::Keep).expect("the promoted scene loads");
    assert_eq!(fields_of(&scene, twin(&scene, game.idle), "Health"), None);
}

/// A scene saved while `Health.current` was an `int`, played under scripts
/// that made it a `float`: the entity the game left alone is not in the
/// save — its value is the scene's, read at today's declaration, as a Rust
/// component's is read at today's type — so an edit the author makes to it
/// in the scene later still reaches the game.
#[test]
fn a_game_save_leaves_out_a_declared_component_only_redeclared_since_its_scene() {
    use khora_data::ecs::{FieldKind, SemanticDomain};
    let game = played();
    let mut fields = super::declared_components::health_fields();
    fields[0] =
        super::declared_components::field("current", FieldKind::Float, ScriptValue::Float(100.0));
    let mut world = World::new();
    super::declared_components::declare(&mut world, "Health", SemanticDomain::Physics, fields);
    super::declared_components::declare(
        &mut world,
        "Mood",
        SemanticDomain::Script,
        super::declared_components::mood_fields(),
    );
    apply(&mut world, &game.base, Identity::Keep).expect("the scene opens");
    let save = capture_save(&world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        !holds_row(&save.changes, game.idle),
        "an entity the game left alone has no row: {:?}",
        save.changes.pages
    );
}
