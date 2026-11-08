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

//! Loads prepared off the frame: read and checked on a worker, against the
//! world's load catalogue, their ids held across frames; committed on the
//! world's thread, where they land exactly as a load prepared with the world
//! does — entities, components, hierarchy and report. A load that fails
//! leaves the world untouched and gives back every id, wherever it ran.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::SceneFile;
use khora_core::script::ScriptValue;
use khora_data::ecs::{FieldKind, Name, Parent, SemanticDomain, Transform};
use khora_data::scene::record::{Record, ReportKind};
use khora_data::scene::snapshot::{prepare_snapshot, prepare_snapshot_with};
use khora_data::scene::{
    capture_save, compose, prepare_game, prepare_game_with, prepare_with, write_scene_file,
    LoadFailure, Prepared,
};

use super::declared_components::{
    attach, declare, declare_behavior, declared_world, entity_field, field, health_fields,
    with_fields, NO_ONE,
};
use super::failures::{fingerprint, occupied_world};
use super::sample::sample_world;
use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// Prepares `record` on a worker, against `world`'s catalogue as it is now,
/// its ids held for it — the way a load runs off the frame.
fn prepare_on_a_worker(world: &World, record: &SceneRecord) -> Result<Prepared, LoadFailure> {
    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    std::thread::scope(|scope| {
        scope
            .spawn(move || prepare_with(&catalog, hold, std::sync::Arc::new(record.clone())))
            .join()
            .expect("the worker panicked")
    })
}

/// A record of eight named, authored entities, the second a child of the
/// first.
fn eight_entities() -> SceneRecord {
    let mut src = World::new();
    let spawned: Vec<EntityId> = (0..8)
        .map(|i| {
            src.spawn((
                Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)),
                Name::new(format!("Incoming {i}")),
            ))
        })
        .collect();
    assert!(src.set_parent(spawned[1], Some(spawned[0])));
    for entity in &spawned {
        src.mark_authored(*entity).expect("authored");
    }
    capture_world(&src).expect("captures")
}

/// [`eight_entities`], made unloadable two ways: a page holding a component
/// nobody knows, and a value that cannot be read.
fn unloadable() -> [(&'static str, SceneRecord); 2] {
    let mut unknown = eight_entities();
    let third = unknown.entities[2];
    let page = isolate(&mut unknown, third, "Transform");
    add_column(&mut unknown, page, "NoSuchComponent", Record::Unit);

    let mut damaged = eight_entities();
    let third = damaged.entities[2];
    *value_mut(&mut damaged, third, "Transform") = Record::Str("not a transform".into());

    [("unknown component", unknown), ("damaged value", damaged)]
}

/// A record of one holder whose behavior names two entities the record does
/// not hold.
fn dangling_record() -> SceneRecord {
    let mut src = World::new();
    declare_behavior(
        &mut src,
        "Holder",
        "ai/holder.erg",
        vec![
            field("first", FieldKind::Entity, ScriptValue::Entity(NO_ONE)),
            field("second", FieldKind::Entity, ScriptValue::Entity(NO_ONE)),
        ],
    );
    let gone_one = src.spawn((Transform::identity(), Name::new("GoneOne")));
    let gone_two = src.spawn((Transform::identity(), Name::new("GoneTwo")));
    let holder = src.spawn((Transform::identity(), Name::new("Holder")));
    attach(
        &mut src,
        holder,
        "Holder",
        with_fields(&[
            ("first", ScriptValue::Entity(gone_one)),
            ("second", ScriptValue::Entity(gone_two)),
        ]),
    );
    src.persistent_id(holder).expect("id");
    let gone = [gone_one, gone_two].map(|e| src.persistent_id(e).expect("id"));
    let mut record = capture_world(&src).expect("captures");
    without_entities(&mut record, &gone);
    record
}

/// **A load prepared on a worker lands as the synchronous load does**: the
/// same entities by identity, the same components, references and
/// hierarchy, the same report — for a world of every kind of component, and
/// one of components declared while the engine runs, in every encoding.
#[test]
fn a_load_prepared_on_a_worker_lands_as_the_synchronous_load_does() {
    let sources = [
        ("every kind", sample_world().0),
        ("declared", declared_world().world),
    ];
    for (kind, src) in &sources {
        let record = capture_world(src).expect("captures");
        for (name, encoding) in every_encoding() {
            let case = format!("{kind}, {name}");
            let back = through(&record, &case, encoding);

            let mut synchronous = World::new();
            let expected = apply(&mut synchronous, &back, Identity::Keep)
                .unwrap_or_else(|e| panic!("{case}: the synchronous load failed: {e}"));

            let mut world = World::new();
            let prepared = prepare_on_a_worker(&world, &back)
                .unwrap_or_else(|e| panic!("{case}: the prepare failed: {e}"));
            let applied = prepared.commit(&mut world, Identity::Keep);

            assert_same_world(src, &world, &case);
            assert_eq!(applied.report, expected.report, "{case}: the report");
            assert_eq!(
                applied
                    .entities
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                expected
                    .entities
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                "{case}: the entities brought in"
            );
            for (_, entity) in &applied.entities {
                assert!(world.contains(*entity), "{case}: {entity:?} is not alive");
            }
        }
    }
}

/// **The same scene prepared on a worker beside a copy of itself takes fresh
/// identities**, as the synchronous load does: four entities, four
/// identities, each copy's hierarchy within itself.
#[test]
fn a_scene_prepared_on_a_worker_beside_itself_takes_fresh_ids() {
    let mut src = World::new();
    let p = src.spawn((Transform::identity(), Name::new("P")));
    let c = src.spawn((Transform::identity(), Name::new("C")));
    assert!(src.set_parent(c, Some(p)));
    src.mark_authored(p).expect("authored");
    src.mark_authored(c).expect("authored");
    let record = capture_world(&src).expect("captures");

    let mut dst = World::new();
    let first = apply(&mut dst, &record, Identity::Keep).expect("the first copy loads");
    let second = prepare_on_a_worker(&dst, &record)
        .expect("the second copy prepares")
        .commit(&mut dst, Identity::Keep);

    assert_eq!(dst.iter_entities().count(), 4);
    let ids: std::collections::HashSet<_> = dst
        .iter_entities()
        .map(|e| dst.persistent_id(e).expect("an id"))
        .collect();
    assert_eq!(ids.len(), 4, "a copy kept an identity the other holds");
    for applied in [&first, &second] {
        let (parent, child) = (applied.entities[0].1, applied.entities[1].1);
        assert_eq!(dst.get::<Parent>(child).map(|x| x.0), Some(parent));
        assert_eq!(
            dst.get::<Children>(parent).map(|l| l.0.clone()),
            Some(vec![child])
        );
    }
}

/// **A reference to nothing prepared on a worker names the world's nowhere
/// entity** — the one the world was made with, so preparing made none — and
/// is reported as the synchronous load reports it.
#[test]
fn a_dead_reference_prepared_on_a_worker_names_the_worlds_nowhere_entity() {
    let record = dangling_record();
    let mut synchronous = World::new();
    let expected = apply(&mut synchronous, &record, Identity::Keep).expect("loads");

    let mut world = World::new();
    let nowhere = world
        .nowhere_entity()
        .expect("a world is made with its nowhere entity");
    let prepared = prepare_on_a_worker(&world, &record).expect("prepares");
    assert_eq!(world.nowhere_entity(), Some(nowhere));
    let applied = prepared.commit(&mut world, Identity::Keep);

    let holder = applied.entities[0].1;
    for name in ["first", "second"] {
        assert_eq!(
            entity_field(&world, holder, "Holder", name),
            nowhere,
            "`{name}` names another entity than the world's nowhere"
        );
    }
    assert!(!world.contains(nowhere));
    assert_eq!(applied.report, expected.report);
    assert!(applied
        .report
        .entries
        .iter()
        .any(|entry| entry.kind == ReportKind::DeadReference));
}

/// **A load prepared before the world's components changed says so**: it
/// carries the generation of the catalogue it was prepared against, which
/// the world's catalogue no longer has. A load prepared after carries the
/// new one.
#[test]
fn a_load_prepared_before_a_declaration_change_carries_the_older_generation() {
    let record = capture_world(&declared_world().world).expect("captures");
    let mut world = World::new();
    let catalog = world.load_catalog();

    let stale = prepare_on_a_worker(&world, &record).expect("prepares");
    assert_eq!(stale.generation(), catalog.generation());

    declare(
        &mut world,
        "Health",
        SemanticDomain::Physics,
        health_fields(),
    );
    let now = world.load_catalog().generation();
    assert_ne!(
        stale.generation(),
        now,
        "a load prepared before the declaration looks current"
    );

    let fresh = prepare_on_a_worker(&world, &record).expect("prepares again");
    assert_eq!(fresh.generation(), now);

    stale.abandon(&mut world);
    fresh.abandon(&mut world);
    assert_eq!(world.iter_entities().count(), 0);
}

/// **A load that fails on a worker leaves the world untouched**: nothing of
/// the record, no identity, the world's own entities as they were — and the
/// failure, message and report, is the synchronous load's.
#[test]
fn a_failed_load_prepared_on_a_worker_leaves_the_world_untouched() {
    for (case, record) in unloadable() {
        let mut synchronous = occupied_world();
        let expected = apply(&mut synchronous, &record, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{case}: the synchronous load must fail"));

        let world = occupied_world();
        let before = fingerprint(&world);
        let failure = prepare_on_a_worker(&world, &record)
            .err()
            .unwrap_or_else(|| panic!("{case}: the load must fail"));

        assert_eq!(failure, expected, "{case}: the failure");
        assert_eq!(fingerprint(&world), before, "{case}: the world changed");
        for id in &record.entities {
            assert_eq!(world.entity_with_id(*id), None, "{case}: {id:?} came in");
        }
    }
}

/// **A load that fails on a worker gives back every id it reserved**: a
/// hundred failed loads of eight entities leave the next spawn a small index.
#[test]
fn failed_loads_prepared_on_a_worker_give_back_every_id() {
    for (case, record) in unloadable() {
        let mut world = World::new();
        for _ in 0..100 {
            assert!(prepare_on_a_worker(&world, &record).is_err(), "{case}");
        }
        let spawned = world.spawn(Transform::identity());
        assert!(spawned.index < 16, "{case}: {spawned:?}");
        assert_eq!(world.entity_count(), 1, "{case}");
    }
}

/// **An abandoned load gives back every id; so does a dropped one.** A
/// hundred loads of eight entities prepared on a worker, then abandoned — or
/// simply dropped — bring nothing in and leave the next spawn a small index,
/// with frames ending in between or not.
#[test]
fn an_abandoned_or_dropped_load_prepared_on_a_worker_gives_back_every_id() {
    let record = eight_entities();
    for (case, abandon) in [("abandoned", true), ("dropped", false)] {
        for frames in [false, true] {
            let mut world = World::new();
            for _ in 0..100 {
                let prepared = prepare_on_a_worker(&world, &record).expect("prepares");
                if abandon {
                    prepared.abandon(&mut world);
                } else {
                    drop(prepared);
                }
                if frames {
                    world.settle_reservations();
                    world.release_unspent();
                }
            }
            assert_eq!(world.entity_count(), 0, "{case}: something came in");
            for id in &record.entities {
                assert_eq!(world.entity_with_id(*id), None, "{case}");
            }
            let spawned = world.spawn(Transform::identity());
            assert!(
                spawned.index < 16,
                "{case}, frames {frames}: a hundred loads kept their ids: {spawned:?}"
            );
        }
    }
}

/// **A load's ids are its own until it commits, however many frames go
/// by.** A load prepared on a worker waits while frames spawn through
/// scripts and directly, each frame ending: no spawn and no reservation is
/// handed one of its ids, and once committed every entity it brings is
/// alive, holding what it was saved with, under its saved hierarchy.
#[test]
fn a_prepared_load_keeps_its_ids_across_frames_until_it_commits() {
    let record = eight_entities();
    let mut world = World::new();
    let prepared = prepare_on_a_worker(&world, &record).expect("prepares");

    let mut handed = Vec::new();
    for _ in 0..5 {
        let reserver = world.reserver();
        let reserved: Vec<EntityId> = (0..16).map(|_| reserver.reserve()).collect();
        world.settle_reservations();
        for &id in reserved.iter().step_by(2) {
            assert!(world.spawn_at(id, Transform::identity()));
        }
        handed.extend(reserved);
        handed.extend((0..16).map(|_| world.spawn(Transform::identity())));
        world.release_unspent();
    }
    let others = world.entity_count();

    let applied = prepared.commit(&mut world, Identity::Keep);

    assert_eq!(world.entity_count(), others + record.entities.len());
    for (id, entity) in &applied.entities {
        assert!(world.contains(*entity), "{id:?} is not alive");
        assert!(
            handed.iter().all(|other| other.index != entity.index),
            "{entity:?} has an index a frame handed out"
        );
        assert_eq!(world.entity_with_id(*id), Some(*entity));
    }
    // Each entity found by the name it was saved with — the record lists
    // them in capture order, not spawn order.
    let named = |name: &str| {
        applied
            .entities
            .iter()
            .map(|(_, entity)| *entity)
            .find(|entity| world.get::<Name>(*entity).is_some_and(|n| n.0 == name))
            .unwrap_or_else(|| panic!("no entity brought in is named `{name}`"))
    };
    let mut names: Vec<String> = applied
        .entities
        .iter()
        .map(|(_, entity)| {
            world
                .get::<Name>(*entity)
                .map(|name| name.0.clone())
                .unwrap_or_else(|| panic!("{entity:?} lost its name"))
        })
        .collect();
    names.sort();
    let mut saved: Vec<String> = (0..8).map(|i| format!("Incoming {i}")).collect();
    saved.sort();
    assert_eq!(names, saved, "the names the load brought in");
    let (root, child) = (named("Incoming 0"), named("Incoming 1"));
    assert_eq!(world.get::<Parent>(child).map(|p| p.0), Some(root));
    assert_eq!(
        world.get::<Children>(root).map(|c| c.0.clone()),
        Some(vec![child])
    );
}

/// **A snapshot prepared on a worker lands as the synchronous restore
/// does**, and a file that is not a snapshot is refused the same way,
/// leaving the world untouched.
#[test]
fn a_snapshot_prepared_on_a_worker_lands_as_the_synchronous_restore_does() {
    let (src, _) = sample_world();
    let file = snapshot_of(&src);
    let mut synchronous = World::new();
    let expected = load_snapshot(&mut synchronous, &file).expect("the snapshot restores");

    let mut world = World::new();
    let prepared = snapshot_on_a_worker(&world, &file).expect("the snapshot prepares");
    let applied = prepared.commit(&mut world, Identity::Keep);

    assert_same_world(&src, &world, "snapshot");
    assert_eq!(applied.report, expected.report);

    let not_a_snapshot =
        write_scene_file(&capture_world(&src).expect("captures"), &CompactEncoding)
            .expect("writes");
    let mut synchronous = World::new();
    let expected = prepare_snapshot(&mut synchronous, &not_a_snapshot)
        .err()
        .expect("a record is no snapshot");
    let world = occupied_world();
    let before = fingerprint(&world);
    let failure = snapshot_on_a_worker(&world, &not_a_snapshot)
        .err()
        .expect("a record is no snapshot");
    assert_eq!(failure, expected);
    assert_eq!(
        fingerprint(&world),
        before,
        "a refused snapshot changed the world"
    );
}

/// Prepares the snapshot `file` for `world` on a worker.
fn snapshot_on_a_worker(world: &World, file: &SceneFile) -> Result<Prepared, LoadFailure> {
    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    std::thread::scope(|scope| {
        scope
            .spawn(move || prepare_snapshot_with(&catalog, hold, std::sync::Arc::new(file.clone())))
            .join()
            .expect("the worker panicked")
    })
}

/// **A game prepared on a worker lands as the synchronous game load
/// does**: the scene with the save's differences on top, its runtime state
/// resumed — and a save that cannot be read is refused without a trace.
#[test]
fn a_game_prepared_on_a_worker_lands_as_the_synchronous_game_load_does() {
    let mut scene = World::new();
    declare_behavior(
        &mut scene,
        "Sentry",
        "ai/sentry.erg",
        vec![field("speed", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    let a = scene.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("A"),
    ));
    let c = scene.spawn((Transform::identity(), Name::new("C")));
    attach(
        &mut scene,
        c,
        "Sentry",
        with_fields(&[("speed", ScriptValue::Float(2.0))]),
    );
    for entity in [a, c] {
        scene.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&scene).expect("captures");
    scene.get_mut::<Transform>(a).expect("placed").translation.x = 5.0;
    let created = scene.spawn((Transform::identity(), Name::new("Spawned in play")));
    assert!(scene.set_parent(created, Some(a)));
    let save = capture_save(&scene, AssetUUID::new(), &base).expect("saves");
    let record = compose(&base, &save);

    let mut synchronous = World::new();
    let expected = prepare_game(&mut synchronous, &record)
        .expect("the game prepares")
        .commit(&mut synchronous, Identity::Keep);

    let mut world = World::new();
    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    let prepared = std::thread::scope(|scope| {
        scope
            .spawn(|| prepare_game_with(&catalog, hold, &record))
            .join()
            .expect("the worker panicked")
    })
    .expect("the game prepares on a worker");
    let applied = prepared.commit(&mut world, Identity::Keep);

    assert_same_world(&synchronous, &world, "game");
    assert_eq!(applied.report, expected.report);

    let mut damaged = record.clone();
    let a_id = synchronous
        .persistent_id(expected.entities[0].1)
        .expect("an id");
    *value_mut(&mut damaged, a_id, "Transform") = Record::Str("garbage".into());
    let world = occupied_world();
    let before = fingerprint(&world);
    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    let refused = std::thread::scope(|scope| {
        scope
            .spawn(|| prepare_game_with(&catalog, hold, &damaged))
            .join()
            .expect("the worker panicked")
    });
    assert!(refused.is_err(), "a damaged game was prepared");
    assert_eq!(
        fingerprint(&world),
        before,
        "a refused game changed the world"
    );
}

/// **A load prepared for one world changes nothing in another.** Its ids
/// were held by the first world's reserver; the second world's live
/// entities may carry the very same ids. Committing it there must not
/// rename, overwrite or move any of them.
#[test]
fn a_load_prepared_for_one_world_leaves_another_world_untouched() {
    let record = eight_entities();
    let elsewhere = World::new();
    let prepared = prepare_on_a_worker(&elsewhere, &record).expect("prepares");

    let mut world = World::new();
    let own: Vec<EntityId> = (0..8)
        .map(|i| world.spawn((Transform::identity(), Name::new(format!("Own {i}")))))
        .collect();
    for entity in &own {
        world.mark_authored(*entity).expect("authored");
    }
    // Both worlds are fresh: the load held the indices after `elsewhere`'s
    // nowhere entity, which are the ones `world` spawned its own on.
    let before = fingerprint(&world);

    let _ = prepared.commit(&mut world, Identity::Keep);

    assert_eq!(
        fingerprint(&world),
        before,
        "a load held by another world's reserver changed this world"
    );
}

/// **A load prepared before a declaration changed lands as a load prepared
/// now would, or not at all.** Prepared while the world knew no `Health` —
/// so the load would register the file's — then the world declares a
/// `Health` of its own with other fields. A load prepared now reads the
/// file's `Health` onto the world's by name and reports every field it
/// drops; the stale load, committed, must not drop them in silence.
#[test]
fn a_load_prepared_before_a_declaration_reports_what_its_commit_drops() {
    let record = capture_world(&declared_world().world).expect("captures");
    let armour = || vec![field("armour", FieldKind::Float, ScriptValue::Float(1.5))];

    let mut current = World::new();
    declare(&mut current, "Health", SemanticDomain::Physics, armour());
    let expected = apply(&mut current, &record, Identity::Keep).expect("loads");
    assert!(
        !expected.report.entries.is_empty(),
        "a load onto the other `Health` reports the fields it drops"
    );

    let mut world = World::new();
    let stale = prepare_on_a_worker(&world, &record).expect("prepares");
    declare(&mut world, "Health", SemanticDomain::Physics, armour());
    assert_ne!(stale.generation(), world.load_catalog().generation());

    let applied = stale.commit(&mut world, Identity::Keep);

    assert_eq!(
        applied.report, expected.report,
        "the stale load dropped the file's `Health` fields without a word"
    );
}

/// **A stale game is prepared again as a game.** A game save keeps the
/// runtime state a behavior resumes with, which a scene load skips. A game
/// prepared on a worker, then committed after the world's components
/// changed, is prepared again the way it was first prepared: it lands as the
/// synchronous game load does, its state companion included.
#[test]
fn a_stale_game_is_prepared_again_as_a_game() {
    let mut scene = World::new();
    declare_behavior(
        &mut scene,
        "Sentry",
        "ai/sentry.erg",
        vec![field("speed", FieldKind::Float, ScriptValue::Float(1.0))],
    );
    let c = scene.spawn((Transform::identity(), Name::new("C")));
    attach(
        &mut scene,
        c,
        "Sentry",
        with_fields(&[("speed", ScriptValue::Float(2.0))]),
    );
    scene.mark_authored(c).expect("authored");
    let base = capture_world(&scene).expect("captures");
    attach(
        &mut scene,
        c,
        &super::declared_components::state_of("Sentry"),
        with_fields(&[("owed", ScriptValue::Float(0.5))]),
    );
    let save = capture_save(&scene, AssetUUID::new(), &base).expect("saves");
    let record = compose(&base, &save);

    let mut synchronous = World::new();
    let expected = prepare_game(&mut synchronous, &record)
        .expect("the game prepares")
        .commit(&mut synchronous, Identity::Keep);

    let mut world = World::new();
    let catalog = world.load_catalog();
    let hold = world.hold_reservations();
    let stale = std::thread::scope(|scope| {
        scope
            .spawn(|| prepare_game_with(&catalog, hold, &record))
            .join()
            .expect("the worker panicked")
    })
    .expect("the game prepares on a worker");
    declare(
        &mut world,
        "Unrelated",
        SemanticDomain::Script,
        health_fields(),
    );
    let applied = stale.commit(&mut world, Identity::Keep);

    assert_same_world(&synchronous, &world, "stale game");
    assert_eq!(applied.report, expected.report);
    let c_back = world
        .entity_with_id(scene.persistent_id(c).expect("an id"))
        .expect("C came back");
    assert_eq!(
        super::declared_components::field_of(
            &world,
            c_back,
            &super::declared_components::state_of("Sentry"),
            "owed"
        ),
        Some(ScriptValue::Float(0.5)),
        "the stale game was prepared again as a scene: its runtime state is gone"
    );
}

/// **A stale snapshot is prepared again as a snapshot**, and lands as the
/// synchronous restore does.
#[test]
fn a_stale_snapshot_is_prepared_again_and_restores() {
    let (src, _) = sample_world();
    let file = snapshot_of(&src);
    let mut world = World::new();
    let stale = snapshot_on_a_worker(&world, &file).expect("the snapshot prepares");
    declare(
        &mut world,
        "Unrelated",
        SemanticDomain::Script,
        health_fields(),
    );
    assert_ne!(stale.generation(), world.load_catalog().generation());
    let applied = stale.commit(&mut world, Identity::Keep);
    let mut synchronous = World::new();
    let expected = load_snapshot(&mut synchronous, &file).expect("restores");
    assert_same_world(&src, &world, "stale snapshot");
    assert_eq!(applied.report, expected.report);
}
