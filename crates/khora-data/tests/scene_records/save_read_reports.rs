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

//! What reading a game save adapts reaches the load's report.
//!
//! Composing a save reads the scene, the save's changes and its before-image
//! as this build reads them, before merging them. A value adapted there — a
//! field widened to today's declaration, a value no field takes dropped — is
//! an adaptation of the load: `compose_reporting` reports it, once, and the
//! composed record holds the adapted value, so the final load has nothing
//! left to adapt.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{FieldKind, SemanticDomain, Transform};
use khora_data::scene::record::{ReportEntry, ReportKind};
use khora_data::scene::{
    capture_save, capture_subtree, collapse, compose_reporting, expand_reporting,
    instantiate_prefab, prepare_game, SaveRecord,
};

use super::declared_components::{
    attach, declare, declare_both, field, field_of, health_fields, mood_fields, set_field,
};
use super::prefab_sample::{save, Library};
use super::renames::Stamina;
use super::*;

/// A scene played: the scene as captured, the save of the run, and the
/// identities the tests follow.
///
/// The scene: `guard` holds a `Health` (`current` 80, `label` "guard"),
/// `idle` holds none. The run: `guard`'s `Health.current` falls to 40 and it
/// moves; `spawned` is made, holding a `Health` (`current` 9).
struct Played {
    base: SceneRecord,
    save: SaveRecord,
    guard: PersistentId,
    idle: PersistentId,
    spawned: PersistentId,
}

/// The run, each `Health.target` naming a live entity — `guard`'s `idle`,
/// `spawned`'s `guard` — so nothing in it is adapted on load.
fn played() -> Played {
    play(true)
}

/// The run; with `targets`, each `Health.target` names a live entity, without
/// them each keeps its declared default, an entity no world holds.
fn play(targets: bool) -> Played {
    let mut world = World::new();
    declare_both(&mut world);
    let guard = world.spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
    let idle = world.spawn(Transform::identity());
    let mut fields = vec![
        ("current".into(), ScriptValue::Int(80)),
        ("label".into(), ScriptValue::Str("guard".into())),
    ];
    if targets {
        fields.push(("target".into(), ScriptValue::Entity(idle)));
    }
    attach(&mut world, guard, "Health", ScriptValue::Struct(fields));
    for entity in [guard, idle] {
        world.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&world).expect("the scene captures");

    set_field(&mut world, guard, "Health", "current", ScriptValue::Int(40));
    world
        .get_mut::<Transform>(guard)
        .expect("placed")
        .translation
        .y = 5.0;
    let spawned = world.spawn(Transform::from_translation(Vec3::new(9.0, 9.0, 9.0)));
    let mut fields = vec![("current".into(), ScriptValue::Int(9))];
    if targets {
        fields.push(("target".into(), ScriptValue::Entity(guard)));
    }
    attach(&mut world, spawned, "Health", ScriptValue::Struct(fields));
    let save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");

    let id = |entity| world.persistent_id(entity).expect("an id");
    Played {
        guard: id(guard),
        idle: id(idle),
        spawned: id(spawned),
        base,
        save,
    }
}

/// The fields of `entity`'s `component` cell in `record`, by name.
fn cell_fields<'r>(
    record: &'r mut SceneRecord,
    entity: PersistentId,
    component: &str,
) -> Vec<(String, &'r mut Record)> {
    match value_mut(record, entity, component) {
        Record::Struct { fields, .. } => fields
            .iter_mut()
            .map(|(name, value)| (name.clone(), value))
            .collect(),
        Record::Map(entries) => entries
            .iter_mut()
            .filter_map(|(name, value)| match name {
                Record::Str(name) => Some((name.clone(), value)),
                _ => None,
            })
            .collect(),
        other => panic!("the `{component}` cell is not a component's fields: {other:?}"),
    }
}

/// Writes, into field `to` of `entity`'s `component` cell in `record`, what
/// its field `from` holds — as a hand edit of the file would: a well-formed
/// value, of another type than `to` declares.
fn copy_field(
    record: &mut SceneRecord,
    entity: PersistentId,
    component: &str,
    from: &str,
    to: &str,
) {
    let mut fields = cell_fields(record, entity, component);
    let value = fields
        .iter()
        .find(|(name, _)| name == from)
        .map(|(_, value)| (**value).clone())
        .unwrap_or_else(|| panic!("the `{component}` cell has no `{from}`"));
    let (_, slot) = fields
        .iter_mut()
        .find(|(name, _)| name == to)
        .unwrap_or_else(|| panic!("the `{component}` cell has no `{to}`"));
    **slot = value;
}

/// The entries of `report` that say `kind` of `entity`'s `component` field
/// `path`.
fn entries_for<'r>(
    report: &'r [ReportEntry],
    entity: PersistentId,
    component: &str,
    path: &str,
    kind: &ReportKind,
) -> Vec<&'r ReportEntry> {
    report
        .iter()
        .filter(|entry| {
            entry.entity == Some(entity)
                && entry.component.as_deref() == Some(component)
                && entry.path == path
                && entry.kind == *kind
        })
        .collect()
}

/// `record` brought into a world that declares nothing: the world, and what
/// that final load reported.
fn load(record: &SceneRecord) -> (World, Vec<ReportEntry>) {
    let mut dst = World::new();
    let prepared =
        prepare_game(&mut dst, record).unwrap_or_else(|e| panic!("the game did not load: {e}"));
    let applied = prepared.commit(&mut dst, Identity::Keep);
    (dst, applied.report.entries)
}

fn twin(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// The scene saved again once its scripts made `Health.current` a `float`,
/// while the save was taken when it was an `int`.
fn resaved_with_a_float_current(base: &SceneRecord) -> SceneRecord {
    let mut fields = health_fields();
    fields[0] = field("current", FieldKind::Float, ScriptValue::Float(100.0));
    let mut resaved = World::new();
    declare(&mut resaved, "Health", SemanticDomain::Physics, fields);
    declare(&mut resaved, "Mood", SemanticDomain::Script, mood_fields());
    apply(&mut resaved, base, Identity::Keep).expect("the scene opens");
    capture_world(&resaved).expect("the scene captures")
}

/// **A field read at the scene's declaration is reported widened.** The save
/// wrote `guard`'s `Health.current` as an `int`; the scene now declares it a
/// `float`. Composing reads the save's `40` as `40.0` and says so — the
/// entity, the component, the field — and the composed value is the float.
/// The final load reads a float as a float: it reports no widening of its own.
#[test]
fn a_save_field_widened_to_its_scenes_declaration_is_reported() {
    let game = played();
    let base = resaved_with_a_float_current(&game.base);

    let (composed, report) =
        compose_reporting(&base, &game.save).unwrap_or_else(|e| panic!("the save composes: {e}"));
    assert!(
        !entries_for(
            &report,
            game.guard,
            "Health",
            "current",
            &ReportKind::Widened
        )
        .is_empty(),
        "reading the save widened `Health.current` but the report does not say so: {report:?}"
    );

    let (dst, final_report) = load(&composed);
    assert_eq!(
        field_of(&dst, twin(&dst, game.guard), "Health", "current"),
        Some(ScriptValue::Float(40.0)),
        "the composed value is the widened one"
    );
    assert!(
        entries_for(
            &final_report,
            game.guard,
            "Health",
            "current",
            &ReportKind::Widened
        )
        .is_empty(),
        "the composed record already holds the float; the final load widens nothing: \
         {final_report:?}"
    );
}

/// **A value no field takes, in the save's changes, is reported dropped —
/// once.** A hand edit wrote text — its `label` — into `spawned`'s `Health.current`, an
/// `int`. Composing drops it and says so; the composed entity holds the
/// field's declared default; and the final load, reading that default, does
/// not report the drop a second time.
#[test]
fn a_value_the_save_changes_cannot_hold_is_reported_dropped_once() {
    let mut game = played();
    copy_field(
        &mut game.save.changes,
        game.spawned,
        "Health",
        "label",
        "current",
    );

    let (composed, report) =
        compose_reporting(&game.base, &game.save).unwrap_or_else(|e| panic!("composes: {e}"));
    let (dst, final_report) = load(&composed);

    let everything: Vec<ReportEntry> = report.iter().chain(&final_report).cloned().collect();
    assert_eq!(
        entries_for(
            &everything,
            game.spawned,
            "Health",
            "current",
            &ReportKind::Dropped
        )
        .len(),
        1,
        "the dropped value is reported exactly once across compose and load; \
         compose said {report:?}, the load said {final_report:?}"
    );
    assert_eq!(
        entries_for(
            &report,
            game.spawned,
            "Health",
            "current",
            &ReportKind::Dropped
        )
        .len(),
        1,
        "the drop happened reading the save, so composing reports it: {report:?}"
    );
    assert_eq!(
        field_of(&dst, twin(&dst, game.spawned), "Health", "current"),
        Some(ScriptValue::Int(100)),
        "the composed value is the field's declared default"
    );
}

/// **A value no field takes, in the save's before-image, is reported.** A
/// hand edit wrote text — its `label` — into the scene's `Health.current` the save kept for
/// `guard`. Composing drops it reading the before-image and says so; the
/// game's change still lands.
#[test]
fn a_value_the_save_before_image_cannot_hold_is_reported_dropped() {
    let mut game = played();
    copy_field(
        &mut game.save.before,
        game.guard,
        "Health",
        "label",
        "current",
    );

    let (composed, report) =
        compose_reporting(&game.base, &game.save).unwrap_or_else(|e| panic!("composes: {e}"));
    assert_eq!(
        entries_for(
            &report,
            game.guard,
            "Health",
            "current",
            &ReportKind::Dropped
        )
        .len(),
        1,
        "reading the before-image dropped `Health.current` but the report does not say so: \
         {report:?}"
    );

    let (dst, _) = load(&composed);
    assert_eq!(
        field_of(&dst, twin(&dst, game.guard), "Health", "current"),
        Some(ScriptValue::Int(40)),
        "the game's change lands over the adapted before-image"
    );
}

/// **A clean save reports nothing.** Composing a save that this build reads
/// exactly as written adds no entry, and the final load of what it composed
/// adds none either.
#[test]
fn a_clean_save_composes_and_loads_with_an_empty_report() {
    let game = played();

    let (composed, report) =
        compose_reporting(&game.base, &game.save).unwrap_or_else(|e| panic!("composes: {e}"));
    assert!(report.is_empty(), "nothing was adapted, yet: {report:?}");
    let (dst, final_report) = load(&composed);
    assert!(
        final_report.is_empty(),
        "nothing was adapted, yet the load says: {final_report:?}"
    );
    assert_eq!(
        field_of(&dst, twin(&dst, game.guard), "Health", "current"),
        Some(ScriptValue::Int(40))
    );
    assert!(dst.entity_with_id(game.idle).is_some());
}

/// **A reference the save names no entity for is reported once.** Each
/// `Health.target` keeps its declared default, an entity no world holds. The
/// load says so for each entity that holds one — once, whether the
/// composing read or the final load is the one to notice it, never both.
#[test]
fn a_dead_reference_in_a_save_is_reported_once_per_entity() {
    let game = play(false);

    let (composed, report) =
        compose_reporting(&game.base, &game.save).unwrap_or_else(|e| panic!("composes: {e}"));
    let (_, final_report) = load(&composed);
    let everything: Vec<ReportEntry> = report.iter().chain(&final_report).cloned().collect();
    for (who, id) in [("guard", game.guard), ("spawned", game.spawned)] {
        assert_eq!(
            entries_for(&everything, id, "Health", "", &ReportKind::DeadReference).len(),
            1,
            "{who}: its dead `Health.target` is reported exactly once; compose said              {report:?}, the load said {final_report:?}"
        );
    }
}

/// Renames field `from` of `entity`'s `component` cell in `record` to `to`,
/// as a file written before the field's rename holds it.
fn written_before_the_rename(
    record: &mut SceneRecord,
    entity: PersistentId,
    component: &str,
    from: &str,
    to: &str,
) {
    match value_mut(record, entity, component) {
        Record::Struct { fields, .. } => {
            let field = fields
                .iter_mut()
                .find(|(name, _)| name == from)
                .unwrap_or_else(|| panic!("the `{component}` cell has no `{from}`"));
            field.0 = to.to_owned();
        }
        other => panic!("the `{component}` cell is not a struct: {other:?}"),
    }
}

/// The entries of `report` — but the save's changes the scene no longer
/// has, which say so by kind — that speak of an entity, or of a component of
/// one, `composed` does not hold.
fn about_what_was_not_loaded<'r>(
    report: &'r [ReportEntry],
    composed: &SceneRecord,
) -> Vec<&'r ReportEntry> {
    report
        .iter()
        .filter(|entry| entry.kind != ReportKind::RemovedFromScene)
        .filter(|entry| match (entry.entity, entry.component.as_deref()) {
            (Some(id), None) => !composed.entities.contains(&id),
            (Some(id), Some(component)) => !composed.pages.iter().any(|page| {
                page.rows.contains(&id) && page.components.iter().any(|name| name == component)
            }),
            (None, _) => false,
        })
        .collect()
}

/// A scene of two entities, both placed, one holding a `Stamina` — the
/// world that captured it, the entity with the stamina, the other, and the
/// scene.
fn stamina_scene() -> (World, EntityId, EntityId, SceneRecord) {
    let mut world = World::new();
    let stamina = world.spawn((
        Transform::identity(),
        Stamina {
            reserve: 7.5,
            regen: 0.25,
        },
    ));
    let kept = world.spawn(Transform::identity());
    for entity in [stamina, kept] {
        world.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&world).expect("the scene captures");
    (world, stamina, kept, base)
}

/// **What reading the scene adapted for an entity the game destroyed is not
/// an adaptation of the load.** The scene was written before `Stamina.reserve`
/// was renamed from `points`; the game destroyed the entity holding it. The
/// loaded world has no such entity, so the report has nothing to say of it.
#[test]
fn an_adaptation_of_an_entity_the_save_destroyed_is_not_reported() {
    let (mut world, stamina, kept, mut base) = stamina_scene();
    let gone = world.persistent_id(stamina).expect("an id");
    world.despawn(stamina);
    world
        .get_mut::<Transform>(kept)
        .expect("placed")
        .translation
        .x = 3.0;
    let save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    written_before_the_rename(&mut base, gone, "Stamina", "reserve", "points");

    let (composed, report) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("composes: {e}"));
    assert!(
        !composed.entities.contains(&gone),
        "the destroyed entity stays gone"
    );
    let stray = about_what_was_not_loaded(&report, &composed);
    assert!(
        stray.is_empty(),
        "the report speaks of what the load did not bring in: {stray:?}"
    );
}

/// **What reading the scene adapted in a component the game removed is not
/// an adaptation of the load.** The scene's `Stamina` was written under its
/// field's former name; the game took the `Stamina` off. The loaded entity
/// holds none, so the report has nothing to say of it.
#[test]
fn an_adaptation_of_a_component_the_save_removed_is_not_reported() {
    let (mut world, stamina, _, mut base) = stamina_scene();
    let id = world.persistent_id(stamina).expect("an id");
    world.remove_component::<Stamina>(stamina).expect("off");
    let save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    written_before_the_rename(&mut base, id, "Stamina", "reserve", "points");

    let (composed, report) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("composes: {e}"));
    let stray = about_what_was_not_loaded(&report, &composed);
    assert!(
        stray.is_empty(),
        "the report speaks of what the load did not bring in: {stray:?}"
    );
}

/// **A change the scene no longer has is reported as such, and only as
/// such.** The save, written before `Stamina.reserve` was renamed from
/// `points`, drained the stamina of an entity the author has since deleted
/// from the scene. The load says the change was dropped; that reading the
/// dropped change renamed a field is not something the loaded world shows.
#[test]
fn a_change_to_an_entity_the_scene_lost_is_reported_only_as_removed_from_scene() {
    let (mut world, stamina, _, base) = stamina_scene();
    let id = world.persistent_id(stamina).expect("an id");
    world.get_mut::<Stamina>(stamina).expect("there").reserve = 1.0;
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    written_before_the_rename(&mut save.changes, id, "Stamina", "reserve", "points");
    written_before_the_rename(&mut save.before, id, "Stamina", "reserve", "points");

    let mut edited = World::new();
    apply(&mut edited, &base, Identity::Keep).expect("the scene opens");
    let lost = edited.entity_with_id(id).expect("there");
    edited.despawn(lost);
    let base = capture_world(&edited).expect("the scene captures");

    let (composed, report) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("composes: {e}"));
    assert!(
        report
            .iter()
            .any(|entry| entry.entity == Some(id) && entry.kind == ReportKind::RemovedFromScene),
        "the dropped change is reported: {report:?}"
    );
    let stray = about_what_was_not_loaded(&report, &composed);
    assert!(
        stray.is_empty(),
        "the report speaks of what the load did not bring in: {stray:?}"
    );
}

/// The prefab `Pack`: one entity holding a `Stamina`, its file written before
/// `Stamina.reserve` was renamed from `points`.
fn pack_written_before_the_rename() -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((
        Transform::identity(),
        Stamina {
            reserve: 7.5,
            regen: 0.25,
        },
    ));
    let id = world.mark_authored(root).expect("authored");
    let mut record = capture_subtree(&world, root).expect("the pack captures");
    written_before_the_rename(&mut record, id, "Stamina", "reserve", "points");
    record
}

/// Whether `report` says some `Stamina` field was read from `points`.
fn says_points_was_renamed(report: &[ReportEntry]) -> bool {
    report.iter().any(|entry| {
        entry.component.as_deref() == Some("Stamina")
            && entry.kind
                == ReportKind::Renamed {
                    from: "points".into(),
                }
    })
}

/// Coverage: **expanding an instance reports what reading its prefab
/// adapted.** A scene holds an instance of `Pack`, whose file predates the
/// field's rename: expanding it reads `points` as `reserve`, and says so.
#[test]
fn expanding_an_instance_reports_what_reading_its_prefab_adapted() {
    let pack_id = AssetUUID::new();
    let library = Library::default().with(pack_id, pack_written_before_the_rename());
    let mut scene = World::new();
    instantiate_prefab(&mut scene, pack_id, &library)
        .unwrap_or_else(|e| panic!("the pack instantiates: {e}"));
    let record = save(&scene, &library);
    assert!(!record.instances.is_empty(), "the scene links to the pack");

    let (_, report) =
        expand_reporting(&record, &library).unwrap_or_else(|e| panic!("expands: {e}"));
    assert!(
        says_points_was_renamed(&report),
        "reading the pack renamed `points`, the report does not say so: {report:?}"
    );
}

/// **Expanding a nested instance reports what reading its prefab adapted.**
/// A scene holds a `Crate`, a prefab holding an instance of `Pack`, whose file
/// predates the field's rename. Expanding the scene reads `points` as
/// `reserve` in the nested pack, as it does for a pack in the scene itself,
/// and says so.
#[test]
fn expanding_a_nested_instance_reports_what_reading_its_prefab_adapted() {
    let pack_id = AssetUUID::new();
    let library = Library::default().with(pack_id, pack_written_before_the_rename());

    let mut outer = World::new();
    let root = outer.spawn(Transform::identity());
    outer.mark_authored(root).expect("authored");
    let nested = instantiate_prefab(&mut outer, pack_id, &library)
        .unwrap_or_else(|e| panic!("the pack instantiates: {e}"));
    outer.set_parent(nested, Some(root));
    let crate_record = collapse(
        &capture_subtree(&outer, root).expect("the crate captures"),
        &library,
    )
    .unwrap_or_else(|e| panic!("the crate collapses: {e}"));
    assert!(
        !crate_record.instances.is_empty(),
        "the crate links to the pack"
    );
    let crate_id = AssetUUID::new();
    let library = library.with(crate_id, crate_record);

    let mut scene = World::new();
    instantiate_prefab(&mut scene, crate_id, &library)
        .unwrap_or_else(|e| panic!("the crate instantiates: {e}"));
    let record = save(&scene, &library);

    let (_, report) =
        expand_reporting(&record, &library).unwrap_or_else(|e| panic!("expands: {e}"));
    assert!(
        says_points_was_renamed(&report),
        "reading the nested pack renamed `points`, the report does not say so: {report:?}"
    );
}

/// Coverage: **what reading the scene adapted is reported once when a game
/// loads on it.** The scene was written before `Stamina.reserve` was renamed
/// from `points`. One entity's stamina the game left alone; the other's it
/// drained, in a save written before the rename too. Each is reported
/// renamed exactly once across composing and the final load — the scene,
/// the save's changes and its before-image all name the same place.
#[test]
fn a_scene_older_than_a_field_rename_reports_it_once_when_a_game_loads_on_it() {
    let mut world = World::new();
    let stamina = || Stamina {
        reserve: 7.5,
        regen: 0.25,
    };
    let rested = world.spawn((Transform::identity(), stamina()));
    let drained = world.spawn((Transform::identity(), stamina()));
    let rested_id = world.mark_authored(rested).expect("authored");
    let drained_id = world.mark_authored(drained).expect("authored");
    let mut base = capture_world(&world).expect("the scene captures");
    world.get_mut::<Stamina>(drained).expect("there").reserve = 1.0;
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    for id in [rested_id, drained_id] {
        written_before_the_rename(&mut base, id, "Stamina", "reserve", "points");
    }
    written_before_the_rename(
        &mut save.changes,
        drained_id,
        "Stamina",
        "reserve",
        "points",
    );
    written_before_the_rename(&mut save.before, drained_id, "Stamina", "reserve", "points");

    let (composed, report) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("composes: {e}"));
    let (dst, final_report) = load(&composed);
    let everything: Vec<ReportEntry> = report.iter().chain(&final_report).cloned().collect();
    let renamed = ReportKind::Renamed {
        from: "points".into(),
    };
    for (who, id) in [("rested", rested_id), ("drained", drained_id)] {
        assert_eq!(
            entries_for(&everything, id, "Stamina", "reserve", &renamed).len(),
            1,
            "{who}: the rename is reported exactly once; compose said {report:?}, \
             the load said {final_report:?}"
        );
    }
    assert_eq!(
        dst.get::<Stamina>(twin(&dst, drained_id))
            .map(|s| s.reserve),
        Some(1.0),
        "the game's change lands"
    );
    assert_eq!(
        dst.get::<Stamina>(twin(&dst, rested_id)).map(|s| s.reserve),
        Some(7.5),
        "the scene's value, read through its former name"
    );
}
