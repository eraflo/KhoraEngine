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

//! The components a behavior replaced, `"Script"` and its runtime state
//! `"ScriptState"`, are retired: a file written before still loads, every
//! other component in, the two skipped and reported.
//!
//! The names are spelled here as data only — what an old file holds — never
//! as the Rust types they once were.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::{Name, Transform};
use khora_data::scene::record::{Record, ReportEntry, ReportKind};
use khora_data::scene::{
    apply_to_prefab, capture_save, capture_subtree, compose_reporting, expand_reporting,
    instantiate_prefab, is_retired, prepare_game, read_save_file, write_save_file, InstanceOf,
    PrefabApply, RemovedComponents, SaveRecord,
};

use super::prefab_sample::{save, Library};
use super::*;

/// The two retired names, as files hold them.
const LEGACY: [&str; 2] = ["Script", "ScriptState"];

/// A legacy script cell, as an old file wrote it: the module, the behavior,
/// and the authored fields.
fn script_cell() -> Record {
    Record::Struct {
        name: "LegacyScript".into(),
        fields: vec![
            ("module".into(), Record::Str("guard.erg".into())),
            ("behavior".into(), Record::Str("Guard".into())),
            ("fields".into(), Record::Seq(Vec::new())),
        ],
    }
}

/// A legacy state cell — nothing this build could read as one: a retired
/// component's cell is skipped, never read.
fn state_cell() -> Record {
    Record::Struct {
        name: "LegacyState".into(),
        fields: vec![
            ("behavior".into(), Record::Str("Guard".into())),
            ("snapshot".into(), Record::Str("not a snapshot".into())),
        ],
    }
}

/// The cell an old file holds under `name`.
fn legacy_cell(name: &str) -> Record {
    if name == "Script" {
        script_cell()
    } else {
        state_cell()
    }
}

/// Whether `world` holds anything under `name` for `entity`: a component
/// registered by that name with a value there, or a declaration of that name.
fn holds_named(world: &World, entity: EntityId, name: &str) -> bool {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .filter(|reg| reg.type_name == name)
        .any(|reg| (reg.to_json)(world, entity).is_some())
        || world.components().key_named(name).is_some()
}

/// The retired entries of `entries` about `entity`'s `name`.
fn retired_about<'r>(
    entries: &'r [ReportEntry],
    entity: PersistentId,
    name: &str,
) -> Vec<&'r ReportEntry> {
    entries
        .iter()
        .filter(|entry| {
            entry.kind == ReportKind::Retired
                && entry.entity == Some(entity)
                && entry.component.as_deref() == Some(name)
        })
        .collect()
}

/// A world of one authored entity, placed and named.
fn one_guard() -> (World, EntityId, PersistentId) {
    let mut world = World::new();
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("Guard"),
    ));
    let id = world.mark_authored(guard).expect("authored");
    (world, guard, id)
}

/// A behavior replaced both: saves may hold them, and say so on load.
#[test]
fn a_script_and_its_state_are_retired_names() {
    for name in LEGACY {
        assert!(is_retired(name), "`{name}` is retired");
    }
}

/// **An old scene loads.** A scene written while entities held a script and
/// its state — the two beside a `Transform` and a `Name` on one entity —
/// loads in every encoding: the entity keeps its placement and its name,
/// holds nothing under either retired name, and the load reports each,
/// once, as retired.
#[test]
fn a_scene_holding_a_script_and_its_state_loads_without_them_and_reports_both() {
    let (src, _, guard) = one_guard();
    let mut record = capture_world(&src).expect("captures");
    let page = isolate(&mut record, guard, "Transform");
    for name in LEGACY {
        add_column(&mut record, page, name, legacy_cell(name));
    }

    for (encoding_name, encoding) in every_encoding() {
        let back = through(&record, encoding_name, encoding);
        let mut world = World::new();
        let applied = apply(&mut world, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{encoding_name}: an old scene loads: {e}"));

        let entity = world
            .entity_with_id(guard)
            .unwrap_or_else(|| panic!("{encoding_name}: the guard loaded"));
        assert_eq!(
            world.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(1.0, 2.0, 3.0)),
            "{encoding_name}: the guard keeps its placement"
        );
        assert_eq!(
            world.get::<Name>(entity).map(|n| n.as_str().to_owned()),
            Some("Guard".to_owned()),
            "{encoding_name}: the guard keeps its name"
        );
        for name in LEGACY {
            assert!(
                !holds_named(&world, entity, name),
                "{encoding_name}: the guard holds nothing under `{name}`"
            );
            assert_eq!(
                retired_about(&applied.report.entries, guard, name).len(),
                1,
                "{encoding_name}: `{name}` is reported retired once: {:?}",
                applied.report
            );
        }
        let again = capture_world(&world).expect("the loaded world captures");
        assert!(
            again.pages.iter().all(|page| page
                .components
                .iter()
                .all(|c| !LEGACY.contains(&c.as_str()))),
            "{encoding_name}: saving again writes neither: {:?}",
            again.pages
        );
    }
}

/// **An old game save loads.** A save taken while the guard ran a script
/// holds its state among the guard's changes, beside the guard's new
/// placement. The game loads: the guard stands where the save left it,
/// holds no state under the retired name, and the load reports it retired.
#[test]
fn a_game_save_holding_a_script_state_loads_without_it_and_reports_it() {
    let (mut world, guard, guard_id) = one_guard();
    let base = capture_world(&world).expect("the scene captures");
    world
        .get_mut::<Transform>(guard)
        .expect("placed")
        .translation = Vec3::new(7.0, 8.0, 9.0);
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    let page = isolate(&mut save.changes, guard_id, "Transform");
    add_column(&mut save.changes, page, LEGACY[1], state_cell());

    let (composed, composing) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("an old save composes: {e}"));
    let mut dst = World::new();
    let prepared =
        prepare_game(&mut dst, &composed).unwrap_or_else(|e| panic!("an old save loads: {e}"));
    let applied = prepared.commit(&mut dst, Identity::Keep);

    let entity = dst.entity_with_id(guard_id).expect("the guard loaded");
    assert_eq!(
        dst.get::<Transform>(entity).map(|t| t.translation),
        Some(Vec3::new(7.0, 8.0, 9.0)),
        "the guard stands where the save left it"
    );
    assert!(
        !holds_named(&dst, entity, LEGACY[1]),
        "the guard holds no state under the retired name"
    );
    let report: Vec<ReportEntry> = composing
        .into_iter()
        .chain(applied.report.entries)
        .collect();
    assert_eq!(
        retired_about(&report, guard_id, LEGACY[1]).len(),
        1,
        "the state is reported retired once: {report:?}"
    );
}

/// Whether `record` names `name` anywhere a load reads a component name: a
/// page, or an instance's differences — its changes, its before-image, what
/// it removed.
fn names_anywhere(record: &SceneRecord, name: &str) -> bool {
    let in_pages = |record: &SceneRecord| {
        record
            .pages
            .iter()
            .any(|page| page.components.iter().any(|c| c == name))
    };
    in_pages(record)
        || record.instances.iter().any(|instance| {
            in_pages(&instance.delta.changes)
                || in_pages(&instance.delta.before)
                || instance
                    .delta
                    .removed
                    .iter()
                    .any(|removed| removed.components.iter().any(|c| c == name))
        })
}

/// A prefab file written while its root and its lamp each ran a script: the
/// root placed and named, the lamp under it, each with a script cell beside
/// its placement. With the lamp's id in the prefab.
fn scripted_post() -> (SceneRecord, PersistentId) {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Post")));
    let lamp = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Lamp"),
    ));
    world.set_parent(lamp, Some(root));
    let root_id = world.mark_authored(root).expect("authored");
    let lamp_id = world.mark_authored(lamp).expect("authored");
    let mut record = capture_subtree(&world, root).expect("the post captures");
    for id in [root_id, lamp_id] {
        let page = isolate(&mut record, id, "Transform");
        add_column(&mut record, page, LEGACY[0], script_cell());
    }
    (record, lamp_id)
}

/// **An old prefab's instance loads.** A prefab file written while its
/// entities ran a script, read from every encoding: a scene holding an
/// instance of it expands and loads, each member holding nothing under the
/// retired name and reported retired once; the scene written again names it
/// nowhere — neither in its pages nor in the instance's differences.
#[test]
fn a_prefab_holding_a_script_loads_its_instance_without_it_and_reports_each_member_once() {
    let (post, lamp) = scripted_post();
    for (encoding_name, encoding) in every_encoding() {
        let prefab = AssetUUID::new();
        let library = Library::default().with(prefab, through(&post, encoding_name, encoding));
        let mut placing = World::new();
        let placed = instantiate_prefab(&mut placing, prefab, &library)
            .unwrap_or_else(|e| panic!("{encoding_name}: the old post instantiates: {e}"));
        let instance = placing.persistent_id(placed).expect("the root has an id");
        let scene = save(&placing, &library);

        let (expanded, expanding) = expand_reporting(&scene, &library)
            .unwrap_or_else(|e| panic!("{encoding_name}: the scene expands: {e}"));
        let mut world = World::new();
        let applied = apply(&mut world, &expanded, Identity::Keep)
            .unwrap_or_else(|e| panic!("{encoding_name}: the scene loads: {e}"));
        let report: Vec<ReportEntry> = expanding
            .into_iter()
            .chain(applied.report.entries)
            .collect();

        for (member, what) in [
            (instance, "root"),
            (PersistentId::within(instance, lamp), "lamp"),
        ] {
            let entity = world
                .entity_with_id(member)
                .unwrap_or_else(|| panic!("{encoding_name}: the {what} loaded"));
            assert!(
                !holds_named(&world, entity, LEGACY[0]),
                "{encoding_name}: the {what} holds nothing under the retired name"
            );
            assert_eq!(
                retired_about(&report, member, LEGACY[0]).len(),
                1,
                "{encoding_name}: the {what}'s script is reported retired once: {report:?}"
            );
        }
        let again = save(&world, &library);
        assert!(
            !names_anywhere(&again, LEGACY[0]),
            "{encoding_name}: the scene written again names the retired script: {again:?}"
        );
    }
}

/// **An old instance's differences load.** A scene written while an
/// instance's lamp ran a script holds that script in the instance's changes
/// and its before-image, beside the lamp's move, and names the retired two
/// among what the game removed from the root. The scene expands and loads:
/// the lamp moved, holds nothing under the retired name, and is reported
/// retired once; written again, the scene names neither.
#[test]
fn an_instance_whose_differences_name_a_script_loads_without_it() {
    let (mut post, lamp) = scripted_post();
    // The prefab, as it is now: no script.
    for page in &mut post.pages {
        if let Some(at) = page.components.iter().position(|c| c == LEGACY[0]) {
            page.components.remove(at);
            page.columns.remove(at);
        }
    }
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, post);
    let mut placing = World::new();
    let placed = instantiate_prefab(&mut placing, prefab, &library).expect("the post instantiates");
    let instance = placing.persistent_id(placed).expect("the root has an id");
    let lamp_id = PersistentId::within(instance, lamp);
    let lamp_entity = placing.entity_with_id(lamp_id).expect("the lamp");
    placing
        .get_mut::<Transform>(lamp_entity)
        .expect("placed")
        .translation = Vec3::new(4.0, 5.0, 6.0);
    let mut scene = save(&placing, &library);
    assert_eq!(scene.instances.len(), 1, "the scene links to the post");
    let delta = &mut scene.instances[0].delta;
    let page = isolate(&mut delta.changes, lamp_id, "Transform");
    add_column(&mut delta.changes, page, LEGACY[0], script_cell());
    let page = isolate(&mut delta.before, lamp_id, "Transform");
    add_column(&mut delta.before, page, LEGACY[0], script_cell());
    delta.removed.push(RemovedComponents {
        entity: instance,
        components: LEGACY.iter().map(|name| (*name).to_owned()).collect(),
    });

    for (encoding_name, encoding) in every_encoding() {
        let back = through(&scene, encoding_name, encoding);
        let (expanded, expanding) = expand_reporting(&back, &library)
            .unwrap_or_else(|e| panic!("{encoding_name}: the scene expands: {e}"));
        let mut world = World::new();
        let applied = apply(&mut world, &expanded, Identity::Keep)
            .unwrap_or_else(|e| panic!("{encoding_name}: the scene loads: {e}"));
        let report: Vec<ReportEntry> = expanding
            .into_iter()
            .chain(applied.report.entries)
            .collect();

        let entity = world.entity_with_id(lamp_id).expect("the lamp loaded");
        assert_eq!(
            world.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(4.0, 5.0, 6.0)),
            "{encoding_name}: the lamp keeps its move"
        );
        assert!(
            !holds_named(&world, entity, LEGACY[0]),
            "{encoding_name}: the lamp holds nothing under the retired name"
        );
        assert_eq!(
            retired_about(&report, lamp_id, LEGACY[0]).len(),
            1,
            "{encoding_name}: the lamp's script is reported retired once: {report:?}"
        );
        assert!(
            world.entity_with_id(instance).is_some(),
            "{encoding_name}: the root loaded"
        );
        let again = save(&world, &library);
        for name in LEGACY {
            assert!(
                !names_anywhere(&again, name),
                "{encoding_name}: the scene written again names `{name}`: {again:?}"
            );
        }
    }
}

/// **An entity the game made with only a script still loads.** A save
/// taken while a spawned entity held only a script — and the guard its
/// state — read back from every encoding: the game loads, the spawned
/// entity is there holding nothing under the retired name, and each retired
/// cell is reported once.
#[test]
fn a_game_save_whose_created_entity_holds_only_a_script_still_loads_it() {
    let (mut world, guard, guard_id) = one_guard();
    let base = capture_world(&world).expect("the scene captures");
    world
        .get_mut::<Transform>(guard)
        .expect("placed")
        .translation = Vec3::new(7.0, 8.0, 9.0);
    let spawned = world.spawn(Transform::identity());
    let spawned_id = world.mark_authored(spawned).expect("authored");
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("the game saves");
    assert!(save.created.contains(&spawned_id), "the save made it");
    let page = isolate(&mut save.changes, spawned_id, "Transform");
    save.changes.pages[page] = PageRecord {
        components: vec![LEGACY[0].to_owned()],
        rows: vec![spawned_id],
        columns: vec![vec![script_cell()]],
    };
    let page = isolate(&mut save.changes, guard_id, "Transform");
    add_column(&mut save.changes, page, LEGACY[1], state_cell());

    for (encoding_name, encoding) in every_encoding() {
        let file = write_save_file(&save, encoding)
            .unwrap_or_else(|e| panic!("{encoding_name}: the save writes: {e}"));
        let back = read_save_file(&file)
            .unwrap_or_else(|e| panic!("{encoding_name}: the save reads: {e}"));
        let (composed, composing) = compose_reporting(&base, &back)
            .unwrap_or_else(|e| panic!("{encoding_name}: an old save composes: {e}"));
        let mut dst = World::new();
        let prepared = prepare_game(&mut dst, &composed)
            .unwrap_or_else(|e| panic!("{encoding_name}: an old save loads: {e}"));
        let applied = prepared.commit(&mut dst, Identity::Keep);
        let report: Vec<ReportEntry> = composing
            .into_iter()
            .chain(applied.report.entries)
            .collect();

        let made = dst
            .entity_with_id(spawned_id)
            .unwrap_or_else(|| panic!("{encoding_name}: the spawned entity loaded: {report:?}"));
        assert!(
            !holds_named(&dst, made, LEGACY[0]),
            "{encoding_name}: it holds nothing under the retired name"
        );
        assert_eq!(
            retired_about(&report, spawned_id, LEGACY[0]).len(),
            1,
            "{encoding_name}: its script is reported retired once: {report:?}"
        );
        let entity = dst.entity_with_id(guard_id).expect("the guard loaded");
        assert_eq!(
            dst.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(7.0, 8.0, 9.0)),
            "{encoding_name}: the guard stands where the save left it"
        );
        assert_eq!(
            retired_about(&report, guard_id, LEGACY[1]).len(),
            1,
            "{encoding_name}: the guard's state is reported retired once: {report:?}"
        );
    }
}

/// **A save that removed the retired two loads.** The scene still holds the
/// guard's script; the save — saying nothing else of the guard — lists the
/// guard's name and the retired two among what the game removed. The game
/// loads: the guard is placed, nameless, holds nothing under either name,
/// and the scene's script is reported retired once.
#[test]
fn a_game_save_removing_a_script_and_its_state_loads() {
    let (world, _, guard_id) = one_guard();
    let mut base = capture_world(&world).expect("the scene captures");
    let page = isolate(&mut base, guard_id, "Transform");
    add_column(&mut base, page, LEGACY[0], script_cell());
    let mut removed = vec!["Name".to_owned()];
    removed.extend(LEGACY.iter().map(|name| (*name).to_owned()));
    let save = SaveRecord {
        base: AssetUUID::new(),
        order: vec![guard_id],
        destroyed: Vec::new(),
        created: Vec::new(),
        removed: vec![RemovedComponents {
            entity: guard_id,
            components: removed,
        }],
        changes: SceneRecord::default(),
        before: SceneRecord::default(),
    };

    let (composed, composing) =
        compose_reporting(&base, &save).unwrap_or_else(|e| panic!("the save composes: {e}"));
    let mut dst = World::new();
    let prepared =
        prepare_game(&mut dst, &composed).unwrap_or_else(|e| panic!("the save loads: {e}"));
    let applied = prepared.commit(&mut dst, Identity::Keep);
    let report: Vec<ReportEntry> = composing
        .into_iter()
        .chain(applied.report.entries)
        .collect();

    let entity = dst.entity_with_id(guard_id).expect("the guard loaded");
    assert!(
        dst.get::<Transform>(entity).is_some(),
        "the guard is placed"
    );
    assert!(
        dst.get::<Name>(entity).is_none(),
        "the game removed its name"
    );
    for name in LEGACY {
        assert!(
            !holds_named(&dst, entity, name),
            "the guard holds nothing under `{name}`"
        );
    }
    assert_eq!(
        retired_about(&report, guard_id, LEGACY[0]).len(),
        1,
        "the scene's script is reported retired once: {report:?}"
    );
}

/// **Applying an override to an old prefab leaves no retired cell.** The
/// lamp of an instance of a prefab written while its entities ran a script
/// is moved and renamed, and that applied to the prefab — as one field, as
/// one component, as the whole instance: the prefab that comes out holds
/// nothing under the retired name, as a prefab written from a world, or by
/// applying the whole instance, does not.
#[test]
fn applying_an_override_to_an_old_prefab_writes_no_retired_cell() {
    let (post, lamp) = scripted_post();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, post);
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the old post instantiates");
    let instance = world.persistent_id(root).expect("the root has an id");
    let lamp_entity = world
        .entity_with_id(PersistentId::within(instance, lamp))
        .expect("the lamp");
    *world.get_mut::<Name>(lamp_entity).expect("named") = Name::new("Beacon");
    world
        .get_mut::<Transform>(lamp_entity)
        .expect("placed")
        .translation = Vec3::new(0.0, 2.0, 0.0);
    let of = InstanceOf { root, prefab };

    for (what, case) in [
        (
            PrefabApply::Field {
                entity: lamp_entity,
                component: "Transform".into(),
                path: vec!["translation".into()],
            },
            "one field",
        ),
        (
            PrefabApply::Component {
                entity: lamp_entity,
                component: "Name".into(),
            },
            "one component",
        ),
        (PrefabApply::Instance, "the whole instance"),
    ] {
        let written = apply_to_prefab(&world, &of, &what, &library)
            .unwrap_or_else(|e| panic!("{case}: the apply is refused: {e}"));
        assert!(
            !names_anywhere(&written, LEGACY[0]),
            "{case}: the prefab written back still holds the retired script: {:?}",
            written.pages
        );
    }
}
