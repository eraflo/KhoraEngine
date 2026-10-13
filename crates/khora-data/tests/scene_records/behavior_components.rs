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

//! A behavior and its state companion in documents (Ergon spec 09): the
//! behavior's component `Guard` is authored, so a scene keeps it; its
//! companion `Guard#state` is the engine's, `Runtime` and resumable, so only
//! a game save keeps it — the Play snapshot drops it, a promotion never
//! writes it into the scene, and a scene holding one anyway loads without
//! it, reported.

use khora_core::asset::AssetUUID;
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform,
};
use khora_data::scene::record::ReportKind;
use khora_data::scene::{
    capture_save, compose, prepare_game, promote, read_save_file, write_save_file, DeclaredRecord,
    SaveRecord,
};

use super::declared_components::{
    attach, companion_fields, declare_behavior as declare_guard, field, field_of, fields_of,
    set_field,
};
use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

const GUARD: &str = "Guard";
const STATE: &str = "Guard#state";

/// `Guard`'s fields, as its behavior declares them.
fn guard_fields() -> Vec<PackedField> {
    vec![
        field("speed", FieldKind::Float, ScriptValue::Float(1.5)),
        field("hp", FieldKind::Int, ScriptValue::Int(10)),
    ]
}

fn register(
    world: &mut World,
    name: &str,
    provenance: ComponentProvenance,
    resumable: bool,
    fields: Vec<PackedField>,
) {
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
            domain: SemanticDomain::Script,
            provenance,
            resumable,
            layout: PackedLayout::new(fields).expect("a valid layout"),
        })
        .unwrap_or_else(|e| panic!("`{name}` registers: {e}"));
}

/// `Guard` and its companion, registered as the scripts' registration
/// registers them — and `Scratch`, engine state no save keeps.
fn declare_behavior(world: &mut World) {
    declare_guard(world, GUARD, "ai/guard.erg", guard_fields());
    register(
        world,
        "Scratch",
        ComponentProvenance::Runtime,
        false,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// A scene played: the scene holds a guard with its `Guard` (speed 3.0); on
/// Play it is given its state companion; the run tunes its speed to 4.0,
/// moves it to `Alert` and gives it a `Scratch`.
struct Played {
    base: SceneRecord,
    world: World,
    guard: PersistentId,
}

fn played() -> Played {
    let mut world = World::new();
    declare_behavior(&mut world);
    let guard = world.spawn(Transform::identity());
    attach(
        &mut world,
        guard,
        GUARD,
        fields(&[("speed", ScriptValue::Float(3.0))]),
    );
    world.mark_authored(guard).expect("authored");
    let base = capture_world(&world).expect("the scene captures");

    attach(
        &mut world,
        guard,
        STATE,
        fields(&[
            ("state", ScriptValue::Str("Patrol".into())),
            ("initialised", ScriptValue::Bool(true)),
            ("spawned", ScriptValue::Bool(true)),
        ]),
    );
    set_field(&mut world, guard, GUARD, "speed", ScriptValue::Float(4.0));
    set_field(
        &mut world,
        guard,
        STATE,
        "state",
        ScriptValue::Str("Alert".into()),
    );
    attach(&mut world, guard, "Scratch", ScriptValue::Unit);

    Played {
        guard: world.persistent_id(guard).expect("an id"),
        base,
        world,
    }
}

fn twin(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the world is {id:?}"))
}

/// Whether any page of `record` lists `component`.
fn lists(record: &SceneRecord, component: &str) -> bool {
    record
        .pages
        .iter()
        .any(|page| page.components.iter().any(|name| name == component))
}

fn declared_names(record: &SceneRecord) -> Vec<&str> {
    record
        .declared
        .iter()
        .map(|declared| declared.name.as_str())
        .collect()
}

/// `base` and `save` composed, brought into a world that declares nothing.
fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

/// Asserts the guard of `world` resumes where the run left it.
fn assert_resumed(world: &World, guard: PersistentId, case: &str) {
    let guard = twin(world, guard);
    assert_eq!(
        field_of(world, guard, GUARD, "speed"),
        Some(ScriptValue::Float(4.0)),
        "{case}: the behavior's field the run tuned"
    );
    assert_eq!(
        field_of(world, guard, STATE, "state"),
        Some(ScriptValue::Str("Alert".into())),
        "{case}: the state the run left it in"
    );
    assert_eq!(
        field_of(world, guard, STATE, "spawned"),
        Some(ScriptValue::Bool(true)),
        "{case}: its lifecycle"
    );
}

/// **A scene keeps the behavior; a game save keeps its state too.** The
/// scene captured mid-play lists `Guard` and declares only it; the game save
/// brings both back, and leaves out engine state that is not resumable.
#[test]
fn a_scene_keeps_a_behavior_and_a_game_save_keeps_its_state_too() {
    let game = played();

    let scene = capture_world(&game.world).expect("the scene captures");
    assert!(lists(&scene, GUARD), "a scene keeps the behavior");
    assert!(!lists(&scene, STATE), "a scene never keeps its state");
    assert!(!lists(&scene, "Scratch"));
    assert_eq!(declared_names(&scene), vec![GUARD]);

    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        slot_of(&save.changes, game.guard, STATE).is_some(),
        "a game save keeps the behavior's state"
    );
    assert!(
        !lists(&save.changes, "Scratch"),
        "engine state that is not resumable stays out of a game save"
    );
    let dst = load(&game.base, &save);
    assert_resumed(&dst, game.guard, "loaded");
    assert!(
        dst.components().key_named("Scratch").is_none(),
        "nothing in the save declares `Scratch`"
    );
}

/// **A fresh world registers the companion as it was**: `Runtime` and
/// resumable, in `Script`, with its fields — whichever encoding wrote the
/// scene and the save.
#[test]
fn a_game_save_registers_the_state_companion_in_a_fresh_world() {
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

        let key = dst
            .components()
            .key_named(STATE)
            .unwrap_or_else(|| panic!("{name}: the save registers `{STATE}`"));
        let vtable = dst.components().vtable(key).expect("a vtable");
        assert_eq!(vtable.provenance, ComponentProvenance::Runtime, "{name}");
        assert!(vtable.resumable, "{name}: registered resumable");
        assert_eq!(vtable.domain, SemanticDomain::Script, "{name}");
        assert_eq!(
            vtable
                .columns
                .packed()
                .expect("packed")
                .fields()
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            companion_fields()
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            "{name}: its fields"
        );

        let guard = dst
            .components()
            .vtable(dst.components().key_named(GUARD).expect("registered"))
            .expect("a vtable");
        assert_eq!(guard.provenance, ComponentProvenance::Authored, "{name}");
        assert!(!guard.resumable, "{name}");
        assert_resumed(&dst, game.guard, name);
    }
}

/// **The Play snapshot drops the state a game save keeps**: Stop restores the
/// behavior's fields as they were on Play, and no state — the next Play
/// starts the behavior afresh.
#[test]
fn the_play_snapshot_drops_the_state_a_game_save_keeps() {
    let mut game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        slot_of(&save.changes, game.guard, STATE).is_some(),
        "a game save of the same world keeps the state"
    );

    let file = snapshot_of(&game.world);
    let played: Vec<EntityId> = game.world.iter_entities().collect();
    for entity in played {
        game.world.despawn(entity);
    }
    load_snapshot(&mut game.world, &file)
        .unwrap_or_else(|e| panic!("the snapshot did not restore: {e}"));

    let guard = twin(&game.world, game.guard);
    assert_eq!(
        field_of(&game.world, guard, GUARD, "speed"),
        Some(ScriptValue::Float(4.0)),
        "the snapshot keeps the behavior's fields"
    );
    assert_eq!(
        fields_of(&game.world, guard, STATE),
        None,
        "the snapshot keeps no state"
    );
}

/// **A promotion never writes the state into the scene**: what the run did
/// to the behavior's fields is promoted, its state is not — the promoted
/// scene neither lists nor declares the companion.
#[test]
fn promoting_a_game_save_never_promotes_the_state_companion() {
    let game = played();
    let save = capture_save(&game.world, AssetUUID::new(), &game.base).expect("saves");
    assert!(
        slot_of(&save.changes, game.guard, STATE).is_some(),
        "the save holds the state a promotion must leave out"
    );

    let promoted = promote(&game.base, &save);
    assert!(!lists(&promoted, STATE), "no page lists `{STATE}`");
    assert!(
        !declared_names(&promoted).contains(&STATE),
        "the promoted scene does not declare `{STATE}`: {:?}",
        declared_names(&promoted)
    );

    let mut dst = World::new();
    apply(&mut dst, &promoted, Identity::Keep)
        .unwrap_or_else(|e| panic!("the promoted scene does not load: {e}"));
    let guard = twin(&dst, game.guard);
    assert_eq!(
        field_of(&dst, guard, GUARD, "speed"),
        Some(ScriptValue::Float(4.0)),
        "the tuned field is promoted"
    );
    assert_eq!(fields_of(&dst, guard, STATE), None);
}

/// A scene that holds a state companion, as a tool could write one: captured
/// from a world where the companion was registered as authored, its
/// declaration then marked resumable.
fn scene_holding_a_companion() -> (SceneRecord, PersistentId) {
    let mut world = World::new();
    register(
        &mut world,
        GUARD,
        ComponentProvenance::Authored,
        false,
        guard_fields(),
    );
    register(
        &mut world,
        STATE,
        ComponentProvenance::Authored,
        false,
        companion_fields(),
    );
    let guard = world.spawn(Transform::identity());
    attach(&mut world, guard, GUARD, ScriptValue::Unit);
    attach(
        &mut world,
        guard,
        STATE,
        fields(&[("state", ScriptValue::Str("Alert".into()))]),
    );
    let id = world.mark_authored(guard).expect("authored");
    let mut record = capture_world(&world).expect("captures");
    assert!(
        lists(&record, STATE),
        "the hand-made scene holds the companion"
    );
    let declared = record
        .declared
        .iter_mut()
        .find(|declared| declared.name == STATE)
        .expect("the scene declares the companion");
    declared.resumable = true;
    (record, id)
}

/// **A scene holding a state companion loads without it, reported**: the
/// behavior's fields load, the companion is skipped as not saved — and its
/// declaration is still read, so a broken one still fails the load.
#[test]
fn a_scene_holding_a_state_companion_loads_it_as_not_saved() {
    let (record, id) = scene_holding_a_companion();

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep)
        .unwrap_or_else(|e| panic!("the scene does not load: {e}"));
    assert!(
        applied.report.entries.iter().any(|entry| {
            entry.kind == ReportKind::NotSaved
                && entry.component.as_deref() == Some(STATE)
                && entry.entity == Some(id)
        }),
        "the skipped companion is reported: {:?}",
        applied.report
    );
    let guard = twin(&dst, id);
    assert!(
        fields_of(&dst, guard, GUARD).is_some(),
        "the behavior loads"
    );
    assert_eq!(
        fields_of(&dst, guard, STATE),
        None,
        "a scene's copy of the state is not read"
    );

    let (mut broken, _) = scene_holding_a_companion();
    let declared = broken
        .declared
        .iter_mut()
        .find(|declared| declared.name == STATE)
        .expect("declared");
    // `state` defaults to `null`, which a `bool` cannot hold.
    declared.fields[0].kind = FieldKind::Bool;
    let mut dst = World::new();
    assert!(
        apply(&mut dst, &broken, Identity::Keep).is_err(),
        "a broken companion declaration fails the load though its rows are skipped"
    );
}

/// **Whether a declaration is resumable survives every encoding.**
#[test]
fn a_resumable_declaration_survives_every_encoding() {
    let (record, _) = scene_holding_a_companion();
    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let resumable = |record: &SceneRecord| {
            record
                .declared
                .iter()
                .map(|declared| (declared.name.clone(), declared.resumable))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            resumable(&back),
            vec![(GUARD.to_owned(), false), (STATE.to_owned(), true)],
            "{name}"
        );
    }
}

/// **A declaration written before `resumable` existed reads as not
/// resumable.**
#[test]
fn a_declaration_written_before_resumable_reads_as_not_resumable() {
    let (record, _) = scene_holding_a_companion();
    // The companion's: one marked resumable, so its flag is written whatever
    // a writer leaves out for `false`.
    let companion = record
        .declared
        .iter()
        .find(|declared| declared.name == STATE)
        .expect("declared");
    let mut older = serde_json::to_value(companion).expect("a declaration serialises");
    let removed = older
        .as_object_mut()
        .expect("a declaration is an object")
        .remove("resumable");
    assert!(removed.is_some(), "a declaration records `resumable`");

    let read: DeclaredRecord =
        serde_json::from_value(older).expect("a declaration written before `resumable` reads");
    assert!(!read.resumable);
    assert_eq!(&read.name, STATE);
}

/// **A prefab of a played entity keeps its behavior, never its state**: a
/// prefab is authored, so made from a guard mid-play it holds `Guard` and
/// declares only it.
#[test]
fn a_prefab_made_mid_play_keeps_the_behavior_and_never_its_state() {
    let game = played();
    let guard = twin(&game.world, game.guard);
    let prefab = khora_data::scene::capture_subtree(&game.world, guard).expect("captures");
    assert!(lists(&prefab, GUARD), "the prefab keeps the behavior");
    assert!(!lists(&prefab, STATE), "the prefab never keeps its state");
    assert_eq!(declared_names(&prefab), vec![GUARD]);
}
