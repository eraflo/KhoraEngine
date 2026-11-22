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

//! Loads prepared with the world, then committed after its components
//! changed: each is prepared again against the components as they are, the
//! way it was first prepared — a scene, a game or a snapshot — and lands as a
//! load prepared at that point would. One refused when prepared again, or
//! committed into another world, adds nothing and gives back its ids.

use khora_core::asset::AssetUUID;
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    Component, ComponentProvenance, FieldKind, PackedField, PackedLayout, SemanticDomain, Transform,
};
use khora_data::scene::record::ReportKind;
use khora_data::scene::snapshot::prepare_snapshot;
use khora_data::scene::{capture_save, compose, prepare, prepare_game, LoadFailure, Prepared};

use super::declared_components::{
    assert_same_declared, declare, declare_both, declared_fields, declared_world, field,
    health_fields, mood_fields, set_field,
};
use super::failures::{fingerprint, occupied_world};
use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// A Rust component named like the file's declared `Mood`: once a world
/// registers it, a file declaring `Mood` is refused.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mood(i32);
impl Component for Mood {}

/// How a load is first prepared: a scene, a game, or a snapshot.
type Preparer = fn(&mut World) -> Result<Prepared, LoadFailure>;

/// The three ways to prepare the declared world, each named.
fn every_preparer() -> [(&'static str, Preparer); 3] {
    fn scene(world: &mut World) -> Result<Prepared, LoadFailure> {
        prepare(
            world,
            capture_world(&declared_world().world).expect("captures"),
        )
    }
    fn game(world: &mut World) -> Result<Prepared, LoadFailure> {
        prepare_game(
            world,
            capture_world(&declared_world().world).expect("captures"),
        )
    }
    fn snapshot(world: &mut World) -> Result<Prepared, LoadFailure> {
        prepare_snapshot(world, snapshot_of(&declared_world().world))
    }
    [("scene", scene), ("game", game), ("snapshot", snapshot)]
}

/// Asserts `applied` brought nothing in and its report ends with `NotLoaded`.
fn assert_not_loaded(applied: &Applied, case: &str) {
    assert!(
        applied.entities.is_empty(),
        "{case}: {:?}",
        applied.entities
    );
    assert!(
        matches!(
            applied.report.entries.last().map(|entry| &entry.kind),
            Some(ReportKind::NotLoaded { .. })
        ),
        "{case}: {:?}",
        applied.report.entries
    );
}

/// `Health` as another script declares it: one field the file's has not.
fn armour() -> Vec<PackedField> {
    vec![field("armour", FieldKind::Float, ScriptValue::Float(1.5))]
}

/// `Health` relaid out by a reload: `current` kept, a new `shield`, the
/// file's other fields gone.
fn relaid_health() -> Vec<PackedField> {
    vec![
        field("current", FieldKind::Int, ScriptValue::Int(100)),
        field("shield", FieldKind::Int, ScriptValue::Int(5)),
    ]
}

/// A world that declares `Health` with `health` and `Mood` as the scripts do.
fn declaring(health: Vec<PackedField>) -> World {
    let mut world = World::new();
    declare(&mut world, "Health", SemanticDomain::Physics, health);
    declare(&mut world, "Mood", SemanticDomain::Script, mood_fields());
    world
}

/// **A scene prepared before its world declared a component lands as one
/// prepared after.** Prepared while the world knew no `Health` — so the load
/// would register the file's — then the world declares a `Health` of its own
/// with other fields. Committed, the load reads the file's `Health` onto the
/// world's by name and reports the fields it drops.
#[test]
fn a_scene_prepared_before_a_declaration_lands_as_one_prepared_after() {
    let record = capture_world(&declared_world().world).expect("captures");
    let mut current = World::new();
    declare(&mut current, "Health", SemanticDomain::Physics, armour());
    let expected = apply(&mut current, &record, Identity::Keep).expect("loads");

    let mut world = World::new();
    let stale = prepare(&mut world, record.clone()).expect("prepares");
    declare(&mut world, "Health", SemanticDomain::Physics, armour());
    assert!(!stale.is_current(&world));
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(
        applied.report, expected.report,
        "the stale scene was committed against the components it was prepared with"
    );
    assert_eq!(declared_fields(&world, "Health"), Some(armour()));
    assert_same_declared(&current, &world, "scene prepared before a declaration");
}

/// **A scene prepared before a component was relaid out lands as one
/// prepared after.** Its `Health` values were staged in the old layout; a
/// reload gives `Health` other fields; committed, each value is read again,
/// field by field, into the new layout.
#[test]
fn a_scene_prepared_before_a_relayout_lands_as_one_prepared_after() {
    let record = capture_world(&declared_world().world).expect("captures");
    let mut current = declaring(relaid_health());
    let expected = apply(&mut current, &record, Identity::Keep).expect("loads");

    let mut world = declaring(health_fields());
    let stale = prepare(&mut world, record.clone()).expect("prepares");
    let key = world.components().key_named("Health").expect("declared");
    world
        .relayout(key, PackedLayout::new(relaid_health()).expect("a layout"))
        .expect("`Health` is relaid out");
    assert!(!stale.is_current(&world));
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(applied.report, expected.report);
    assert_same_declared(&current, &world, "scene prepared before a relayout");
}

/// **A game prepared before its world declared a component is prepared again
/// as a game**: it lands as a game prepared after the declaration does.
#[test]
fn a_game_prepared_before_a_declaration_lands_as_one_prepared_after() {
    let mut played = declared_world();
    let base = capture_world(&played.world).expect("captures");
    set_field(
        &mut played.world,
        played.a,
        "Health",
        "current",
        ScriptValue::Int(7),
    );
    let save = capture_save(&played.world, AssetUUID::new(), &base).expect("saves");
    let record = compose(&base, &save);

    let mut current = World::new();
    declare(&mut current, "Health", SemanticDomain::Physics, armour());
    let expected = prepare_game(&mut current, record.clone())
        .expect("the game prepares")
        .commit(&mut current, Identity::Keep);

    let mut world = World::new();
    let stale = prepare_game(&mut world, record.clone()).expect("the game prepares");
    declare(&mut world, "Health", SemanticDomain::Physics, armour());
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(applied.report, expected.report);
    assert_eq!(declared_fields(&world, "Health"), Some(armour()));
    assert_same_declared(&current, &world, "game prepared before a declaration");
}

/// **A snapshot prepared before a component was relaid out is prepared again
/// as a snapshot**: it restores as a snapshot prepared after the relayout does.
#[test]
fn a_snapshot_prepared_before_a_relayout_lands_as_one_prepared_after() {
    let file = snapshot_of(&declared_world().world);
    let mut current = declaring(relaid_health());
    let expected = load_snapshot(&mut current, &file).expect("restores");

    let mut world = World::new();
    declare_both(&mut world);
    let stale = prepare_snapshot(&mut world, file.clone()).expect("the snapshot prepares");
    let key = world.components().key_named("Health").expect("declared");
    world
        .relayout(key, PackedLayout::new(relaid_health()).expect("a layout"))
        .expect("`Health` is relaid out");
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(applied.report, expected.report);
    assert_same_declared(&current, &world, "snapshot prepared before a relayout");
}

/// **A stale load that fails when prepared again leaves the world as it
/// was.** Prepared while the world knew no `Mood`, then the world registers
/// a Rust component of that name: a load prepared now is refused, the file
/// declaring a name an engine component has. Committed, the stale load —
/// scene, game or snapshot — brings nothing in, registers none of the file's
/// declarations, and says why.
#[test]
fn a_stale_load_that_fails_when_prepared_again_leaves_the_world_as_it_was() {
    for (case, prepare_load) in every_preparer() {
        let mut world = occupied_world();
        let stale = prepare_load(&mut world).expect("prepares");
        world.register_component::<Mood>(SemanticDomain::Script);
        assert!(prepare_load(&mut world).is_err(), "{case}: a fresh load");
        let before = fingerprint(&world);

        let applied = stale.commit(&mut world, Identity::Keep);

        assert_not_loaded(&applied, case);
        assert_eq!(fingerprint(&world), before, "{case}: the world changed");
        assert_eq!(world.components().key_named("Health"), None, "{case}");
    }
}

/// **Stale loads that fail when prepared again give back every id.** A
/// hundred stale loads of four entities each hold four hundred ids; once
/// committed and refused, four hundred spawns reuse those slots rather than
/// grow the store past them.
#[test]
fn stale_loads_that_fail_when_prepared_again_give_back_every_id() {
    for (case, prepare_load) in every_preparer() {
        let mut world = World::new();
        let stale: Vec<Prepared> = (0..100)
            .map(|_| prepare_load(&mut world).expect("prepares"))
            .collect();
        world.register_component::<Mood>(SemanticDomain::Script);
        for load in stale {
            assert_not_loaded(&load.commit(&mut world, Identity::Keep), case);
        }
        world.release_unspent();

        let highest = (0..400)
            .map(|_| world.spawn(Transform::identity()).index)
            .max();
        assert!(
            highest < Some(420),
            "{case}: the stale loads kept their ids: {highest:?}"
        );
    }
}

/// **A stale load prepared again takes no more ids than it brings.** Fifty
/// times over, a load of four entities is prepared, the world declares a
/// component, the load commits and its entities are despawned: the store
/// does not grow by the ids each first preparation held.
#[test]
fn a_stale_load_prepared_again_reuses_the_ids_it_held() {
    for (case, prepare_load) in every_preparer() {
        let mut world = World::new();
        for round in 0..50 {
            let stale = prepare_load(&mut world).expect("prepares");
            declare(
                &mut world,
                &format!("Unrelated{round}"),
                SemanticDomain::Script,
                mood_fields(),
            );
            let applied = stale.commit(&mut world, Identity::Keep);
            assert_eq!(applied.entities.len(), 4, "{case}: {:?}", applied.report);
            for (_, entity) in applied.entities {
                world.despawn(entity);
            }
            world.release_unspent();
        }

        let spawned = world.spawn(Transform::identity());
        assert!(
            spawned.index < 32,
            "{case}: re-preparing leaked ids: {spawned:?}"
        );
    }
}

/// **A stale load committed into another world adds nothing there**, even
/// though it would be prepared again: a load is for the world whose
/// reserver holds its ids, stale or not.
#[test]
fn a_stale_load_committed_into_another_world_adds_nothing() {
    for (case, prepare_load) in every_preparer() {
        let mut prepared_in = World::new();
        let stale = prepare_load(&mut prepared_in).expect("prepares");
        declare(
            &mut prepared_in,
            "Unrelated",
            SemanticDomain::Script,
            mood_fields(),
        );
        let mut other = occupied_world();
        let before = fingerprint(&other);

        let applied = stale.commit(&mut other, Identity::Keep);

        assert_not_loaded(&applied, case);
        assert_eq!(
            fingerprint(&other),
            before,
            "{case}: the other world changed"
        );
        assert_eq!(other.components().key_named("Health"), None, "{case}");
    }
}

/// **A scene prepared before a component moved to another domain lands as
/// one prepared after**: `Health` is read into the domain the world keeps it
/// in now.
#[test]
fn a_scene_prepared_before_a_domain_move_lands_as_one_prepared_after() {
    let record = capture_world(&declared_world().world).expect("captures");
    let mut current = World::new();
    declare_both(&mut current);
    let key = current.components().key_named("Health").expect("declared");
    current
        .move_component_domain(key, SemanticDomain::Script)
        .expect("`Health` moves");
    let expected = apply(&mut current, &record, Identity::Keep).expect("loads");

    let mut world = World::new();
    declare_both(&mut world);
    let stale = prepare(&mut world, record.clone()).expect("prepares");
    let key = world.components().key_named("Health").expect("declared");
    world
        .move_component_domain(key, SemanticDomain::Script)
        .expect("`Health` moves");
    assert!(!stale.is_current(&world));
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(applied.report, expected.report);
    assert_same_declared(&current, &world, "scene prepared before a domain move");
    assert_same_world(&current, &world, "scene prepared before a domain move");
}

/// **A scene prepared before a component became runtime state lands as one
/// prepared after**: a scene does not keep `Health` once the world keeps it
/// as resumable runtime state, and the stale scene skips and reports it too.
#[test]
fn a_scene_prepared_before_a_provenance_change_lands_as_one_prepared_after() {
    let record = capture_world(&declared_world().world).expect("captures");
    let mut current = World::new();
    declare_both(&mut current);
    let key = current.components().key_named("Health").expect("declared");
    current
        .set_provenance(key, ComponentProvenance::Runtime, true)
        .expect("`Health` becomes runtime state");
    let expected = apply(&mut current, &record, Identity::Keep).expect("loads");
    assert!(
        expected
            .report
            .entries
            .iter()
            .any(|entry| entry.kind == ReportKind::NotSaved),
        "a scene skips runtime state"
    );

    let mut world = World::new();
    declare_both(&mut world);
    let stale = prepare(&mut world, record.clone()).expect("prepares");
    let key = world.components().key_named("Health").expect("declared");
    world
        .set_provenance(key, ComponentProvenance::Runtime, true)
        .expect("`Health` becomes runtime state");
    assert!(!stale.is_current(&world));
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(applied.report, expected.report);
    assert_same_declared(
        &current,
        &world,
        "scene prepared before a provenance change",
    );
}
