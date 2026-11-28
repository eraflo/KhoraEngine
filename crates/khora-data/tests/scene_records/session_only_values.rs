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

//! Values only the session holds (a mesh made in code): a prefab file leaves
//! them out and says so, and a file holding one anyway is refused it.

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_data::ecs::{AssetRef, Name, Transform};
use khora_data::scene::record::{ReportEntry, ReportKind};
use khora_data::scene::{serialize_prefab_reporting, serialize_subtree_reporting, NoPrefabs};

use super::*;

/// The `NotSaved` entry a save gives for `entity`'s mesh made in code.
fn mesh_left_out(entity: PersistentId) -> ReportEntry {
    ReportEntry {
        entity: Some(entity),
        component: Some("AssetRef<Mesh>".to_owned()),
        path: String::new(),
        kind: ReportKind::NotSaved,
    }
}

/// `record` with the `AssetRef<Mesh>` of the entity known as `id` rewritten to name a
/// mesh made in code, as no save writes it.
fn forge_made(record: &mut SceneRecord, id: PersistentId) {
    match value_mut(record, id, "AssetRef<Mesh>") {
        Record::Variant { variant, .. } => *variant = "Made".to_owned(),
        other => panic!("a mesh reference is recorded as a variant, got {other:?}"),
    }
}

/// A crate prefab: a root and one member drawn with an authored mesh, by its
/// ids in the prefab.
struct Crate {
    record: SceneRecord,
    member: PersistentId,
    mesh: AssetRef<Mesh>,
}

/// Returns the crate prefab, captured from a world of its own.
fn crate_prefab() -> Crate {
    let mut world = World::new();
    let mesh = AssetRef::<Mesh>::Asset(AssetUUID::new());
    let root = world.spawn((Transform::identity(), Name::new("Crate")));
    let member = world.spawn((Transform::identity(), Name::new("Lid"), mesh.clone()));
    world.set_parent(member, Some(root));
    world.mark_authored(root).expect("authored");
    let member = world.mark_authored(member).expect("authored");
    Crate {
        record: khora_data::scene::capture_subtree(&world, root).expect("the crate captures"),
        member,
        mesh,
    }
}

/// A level holding an instance of `prefab`, the instance's lid given a mesh
/// made in code: the world, the level, the lid and the mesh it was given.
fn level_with_made_lid(
    lid: PersistentId,
    prefab: AssetUUID,
    library: &dyn khora_data::scene::PrefabSource,
) -> (World, EntityId, EntityId, AssetRef<Mesh>) {
    let mut world = World::new();
    let level = world.spawn((Transform::identity(), Name::new("Level")));
    world.mark_authored(level).expect("authored");
    let root = khora_data::scene::instantiate_prefab(&mut world, prefab, library)
        .expect("the crate instantiates");
    world.set_parent(root, Some(level));
    let instance = world.persistent_id(root).expect("an id");
    let lid = world
        .entity_with_id(PersistentId::within(instance, lid))
        .expect("the instance's lid");
    let made = AssetRef::<Mesh>::Made(AssetUUID::new());
    assert!(
        world.set_component(lid, made.clone()),
        "the lid is remeshed"
    );
    (world, level, lid, made)
}

/// A prefab file written from a subtree holding a mesh made in code leaves the
/// mesh out and reports it, whether it is written as a prefab or a copy.
#[test]
fn a_prefab_file_reports_the_mesh_made_in_code_it_leaves_out() {
    let mut world = World::new();
    let root = world.spawn((
        Transform::identity(),
        Name::new("Made"),
        AssetRef::<Mesh>::Made(AssetUUID::new()),
    ));
    let id = world.mark_authored(root).expect("authored");

    let (_, as_prefab) = serialize_prefab_reporting(&world, root, AssetUUID::new(), &NoPrefabs)
        .expect("the prefab is written");
    let (_, as_copy) =
        serialize_subtree_reporting(&world, root, &NoPrefabs).expect("the subtree is written");

    assert_eq!(as_prefab, [mesh_left_out(id)], "written as a prefab");
    assert_eq!(as_copy, [mesh_left_out(id)], "written as a copy");
}

/// A mesh made in code written into a scene record (by hand: no save writes
/// one) names nothing a load can find. The load drops it from its entity and
/// reports it `Dropped`; the entity keeps its other components, and its page
/// neighbour keeps its own mesh.
#[test]
fn a_mesh_made_in_code_written_into_a_record_is_dropped_and_reported() {
    let mut src = World::new();
    let authored_mesh = AssetRef::<Mesh>::Asset(AssetUUID::new());
    let kept = src.spawn((
        Transform::identity(),
        Name::new("Kept"),
        authored_mesh.clone(),
    ));
    let forged = src.spawn((
        Transform::identity(),
        Name::new("Forged"),
        AssetRef::<Mesh>::Asset(AssetUUID::new()),
    ));
    let kept_id = src.mark_authored(kept).expect("authored");
    let forged_id = src.mark_authored(forged).expect("authored");
    let mut record = capture_world(&src).expect("captures");
    match value_mut(&mut record, forged_id, "AssetRef<Mesh>") {
        Record::Variant { variant, .. } => *variant = "Made".to_owned(),
        other => panic!("a mesh reference is recorded as a variant, got {other:?}"),
    }

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    let forged = dst
        .entity_with_id(forged_id)
        .expect("the forged entity loads");
    let kept = dst.entity_with_id(kept_id).expect("the kept entity loads");
    assert_eq!(
        dst.get::<AssetRef<Mesh>>(forged),
        None,
        "the load kept a mesh made in code read from a file"
    );
    assert_eq!(
        dst.get::<Name>(forged).map(Name::as_str),
        Some("Forged"),
        "the entity lost its other components"
    );
    assert_eq!(
        dst.get::<AssetRef<Mesh>>(kept),
        Some(&authored_mesh),
        "its page neighbour lost its mesh"
    );
    assert!(
        applied.report.entries.contains(&ReportEntry {
            entity: Some(forged_id),
            component: Some("AssetRef<Mesh>".to_owned()),
            path: String::new(),
            kind: ReportKind::Dropped,
        }),
        "the load does not report the mesh it refused: {:?}",
        applied.report.entries
    );
}

/// A game save leaves out a mesh made in code given during play, and reports
/// it `NotSaved` on its entity.
#[test]
fn capture_save_reporting_reports_a_mesh_made_in_code_left_out() {
    let mut world = World::new();
    let hero = world.spawn((Transform::identity(), Name::new("Hero")));
    let hero_id = world.mark_authored(hero).expect("authored");
    let base = capture_world(&world).expect("the scene captures");
    world
        .add_component(hero, AssetRef::<Mesh>::Made(AssetUUID::new()))
        .expect("the hero takes a mesh");

    let (save, left_out) =
        khora_data::scene::capture_save_reporting(&world, AssetUUID::new(), &base)
            .expect("the game captures");

    assert!(
        !save
            .changes
            .pages
            .iter()
            .any(|page| page.components.iter().any(|name| name == "AssetRef<Mesh>")),
        "the game save holds the mesh made in code"
    );
    assert_eq!(left_out, [mesh_left_out(hero_id)]);
}

/// **A copy within the session keeps what only the session holds.** Copying a
/// subtree holding a mesh made in code gives a copy naming the same mesh, and
/// the copy reports nothing.
#[test]
fn copying_a_subtree_keeps_its_mesh_made_in_code() {
    let mut world = World::new();
    let made = AssetRef::<Mesh>::Made(AssetUUID::new());
    let root = world.spawn((Transform::identity(), Name::new("Made"), made.clone()));
    world.mark_authored(root).expect("authored");

    let (copy, report) =
        khora_data::scene::copy_subtree(&mut world, root, &NoPrefabs).expect("the subtree copies");

    assert_ne!(copy, root, "the copy is another entity");
    assert_eq!(
        world.get::<AssetRef<Mesh>>(copy),
        Some(&made),
        "the copy lost the mesh made in code"
    );
    assert!(report.is_clean(), "the copy reported {:?}", report.entries);
}

/// An entity whose only component is a mesh made in code, read from a file,
/// still loads, under its identity and with no component, the mesh reported
/// `Dropped`.
#[test]
fn an_entity_holding_only_a_mesh_made_in_code_loads_bare() {
    let mut src = World::new();
    let only = src.spawn(AssetRef::<Mesh>::Asset(AssetUUID::new()));
    let id = src.mark_authored(only).expect("authored");
    let mut record = capture_world(&src).expect("captures");
    forge_made(&mut record, id);

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    let loaded = dst
        .entity_with_id(id)
        .expect("the entity holding only a mesh made in code is not loaded");
    assert_eq!(dst.get::<AssetRef<Mesh>>(loaded), None);
    assert_eq!(dst.iter_entities().count(), 1, "one entity loads");
    assert!(
        applied
            .report
            .entries
            .iter()
            .any(|entry| entry.entity == Some(id) && entry.kind == ReportKind::Dropped),
        "the load does not report the mesh it refused: {:?}",
        applied.report.entries
    );
}

/// A scene saved from an entity whose only component is a mesh made in code
/// keeps the entity, and a load of it gives the entity back, bare.
#[test]
fn an_entity_holding_only_a_mesh_made_in_code_is_saved_and_loads_bare() {
    let mut src = World::new();
    let only = src.spawn(AssetRef::<Mesh>::Made(AssetUUID::new()));
    let id = src.mark_authored(only).expect("authored");

    let record = capture_world(&src).expect("captures");
    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    assert!(record.entities.contains(&id), "the entity is not recorded");
    assert!(
        dst.entity_with_id(id).is_some(),
        "the entity saved without its mesh made in code does not load"
    );
}

/// A page whose middle row names a mesh made in code loads its neighbours as
/// written: each entity under its identity, under its parent, the parent's
/// children in their saved order, and the refused row keeps its parent.
#[test]
fn a_refused_mesh_leaves_its_page_neighbours_ids_parents_and_order() {
    let mut src = World::new();
    let parent = src.spawn((Transform::identity(), Name::new("Parent")));
    let meshes: Vec<AssetRef<Mesh>> = (0..3)
        .map(|_| AssetRef::<Mesh>::Asset(AssetUUID::new()))
        .collect();
    let children: Vec<EntityId> = meshes
        .iter()
        .enumerate()
        .map(|(n, mesh)| {
            let child = src.spawn((
                Transform::identity(),
                Name::new(format!("Child {n}")),
                mesh.clone(),
            ));
            src.set_parent(child, Some(parent));
            child
        })
        .collect();
    src.mark_authored(parent).expect("authored");
    let ids: Vec<PersistentId> = children
        .iter()
        .map(|child| src.mark_authored(*child).expect("authored"))
        .collect();
    let mut record = capture_world(&src).expect("captures");
    forge_made(&mut record, ids[1]);

    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    let parent = dst
        .entity_with_id(src.persistent_id(parent).expect("an id"))
        .expect("the parent loads");
    let loaded: Vec<EntityId> = ids
        .iter()
        .map(|id| dst.entity_with_id(*id).expect("each child loads"))
        .collect();
    for (n, child) in loaded.iter().enumerate() {
        assert_eq!(
            dst.get::<Name>(*child).map(Name::as_str),
            Some(format!("Child {n}").as_str()),
            "child {n} took another's values"
        );
        assert_eq!(
            dst.get::<khora_data::ecs::Parent>(*child).map(|p| p.0),
            Some(parent),
            "child {n} lost its parent"
        );
    }
    assert_eq!(dst.get::<AssetRef<Mesh>>(loaded[0]), Some(&meshes[0]));
    assert_eq!(dst.get::<AssetRef<Mesh>>(loaded[1]), None);
    assert_eq!(dst.get::<AssetRef<Mesh>>(loaded[2]), Some(&meshes[2]));
    assert_eq!(
        dst.get::<Children>(parent).map(|c| c.0.clone()),
        Some(loaded),
        "the parent's children are not in their saved order"
    );
}

/// A game save over a scene whose entity holds an authored mesh, the running
/// world having given it a mesh made in code: the save holds the entity's
/// other changes, and a load of it gives the entity with no mesh.
#[test]
fn a_game_save_replacing_an_authored_mesh_with_one_made_in_code_loads_without_a_mesh() {
    let mut world = World::new();
    let authored = AssetRef::<Mesh>::Asset(AssetUUID::new());
    let hero = world.spawn((Transform::identity(), Name::new("Hero"), authored.clone()));
    let hero_id = world.mark_authored(hero).expect("authored");
    let base = capture_world(&world).expect("the scene captures");
    assert!(
        world.set_component(hero, AssetRef::<Mesh>::Made(AssetUUID::new())),
        "the hero's mesh is replaced"
    );
    world.get_mut::<Name>(hero).expect("named").0 = "Hero, played".into();

    let save = khora_data::scene::capture_save(&world, AssetUUID::new(), &base)
        .expect("the game captures");
    let composed = khora_data::scene::compose(&base, &save);
    let mut loaded = World::new();
    khora_data::scene::prepare_game(&mut loaded, composed)
        .expect("the game prepares")
        .commit(&mut loaded, Identity::Keep);

    let hero = loaded.entity_with_id(hero_id).expect("the hero loads");
    assert_eq!(
        loaded.get::<Name>(hero).map(Name::as_str),
        Some("Hero, played"),
        "the hero's other change is lost"
    );
    assert_eq!(
        loaded.get::<AssetRef<Mesh>>(hero),
        None,
        "the load gave the hero a mesh the running world no longer had"
    );
}

/// A prefab file whose member names a mesh made in code (by hand: no save
/// writes one) is instantiated with that member, without the mesh.
#[test]
fn a_prefab_member_naming_a_mesh_made_in_code_is_instantiated_without_it() {
    let mut prefab = crate_prefab();
    forge_made(&mut prefab.record, prefab.member);
    let id = AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(id, prefab.record.clone());
    let mut world = World::new();

    let root = khora_data::scene::instantiate_prefab(&mut world, id, &library)
        .expect("the prefab instantiates");

    let instance = world.persistent_id(root).expect("an id");
    let lid = world
        .entity_with_id(PersistentId::within(instance, prefab.member))
        .expect("the member is instantiated");
    assert_eq!(world.get::<AssetRef<Mesh>>(lid), None);
    assert_eq!(world.get::<Name>(lid).map(Name::as_str), Some("Lid"));
}

/// Copying a level that holds a prefab instance whose member was given a
/// mesh made in code keeps that mesh on the copy's member, the copy an
/// instance of the same prefab.
#[test]
fn copying_an_instance_keeps_a_mesh_made_in_code_on_its_member() {
    let prefab = crate_prefab();
    let id = AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(id, prefab.record.clone());
    let (mut world, level, _, made) = level_with_made_lid(prefab.member, id, &library);

    let (copy, report) =
        khora_data::scene::copy_subtree(&mut world, level, &library).expect("the level copies");

    let copied_root = world
        .get::<Children>(copy)
        .expect("the copy has its crate")
        .0[0];
    assert_eq!(
        world.get::<khora_data::ecs::PrefabInstance>(copied_root),
        Some(&khora_data::ecs::PrefabInstance { prefab: id }),
        "the copied crate is no longer an instance"
    );
    let instance = world.persistent_id(copied_root).expect("an id");
    let lid = world
        .entity_with_id(PersistentId::within(instance, prefab.member))
        .expect("the copy's lid");
    assert_eq!(
        world.get::<AssetRef<Mesh>>(lid),
        Some(&made),
        "the copy's lid lost its mesh made in code; the copy reported {:?}",
        report.entries
    );
    assert!(report.is_clean(), "the copy reported {:?}", report.entries);
}

/// A scene saved from a level whose prefab instance member was given a mesh
/// made in code reports it `NotSaved`, and a load of the scene gives the
/// member back without a mesh, as a game save does.
#[test]
fn a_scene_saved_over_an_instance_member_made_in_code_loads_it_without_a_mesh() {
    let prefab = crate_prefab();
    let id = AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(id, prefab.record.clone());
    let (world, _, lid, _) = level_with_made_lid(prefab.member, id, &library);
    let lid_id = world.persistent_id(lid).expect("an id");

    let (captured, left_out) =
        khora_data::scene::capture_world_reporting(&world).expect("the scene captures");
    let scene = khora_data::scene::collapse(&captured, &library).expect("the scene collapses");
    let loaded = super::prefab_sample::open(&scene, &library);

    assert_eq!(left_out, [mesh_left_out(lid_id)]);
    let lid = loaded.entity_with_id(lid_id).expect("the lid loads");
    assert_eq!(
        loaded.get::<AssetRef<Mesh>>(lid),
        None,
        "the lid took the prefab's mesh back, {:?}",
        prefab.mesh
    );
}

/// Applying a whole instance to its prefab, a member holding a mesh made in
/// code, never strips the prefab's own mesh from that member: no file can
/// hold the made one, and the apply says nothing of it.
#[test]
fn applying_an_instance_with_a_mesh_made_in_code_keeps_the_prefabs_mesh() {
    let prefab = crate_prefab();
    let id = AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(id, prefab.record.clone());
    let (world, _, lid, _) = level_with_made_lid(prefab.member, id, &library);
    let instance = khora_data::scene::instance_of(&world, lid, &library).expect("an instance");

    let applied = khora_data::scene::apply_to_prefab(
        &world,
        &instance,
        &khora_data::scene::PrefabApply::Instance,
        &library,
    )
    .expect("the instance applies");

    assert!(
        slot_of(&applied, prefab.member, "AssetRef<Mesh>").is_some(),
        "the prefab's lid lost its mesh {:?}, with no report",
        prefab.mesh
    );
}

/// A scene whose prefab instance overrides a member's mesh with one made in
/// code (by hand: no save writes one) loads that member without the made
/// mesh, and the load reports it `Dropped` on the member.
#[test]
fn an_instance_override_naming_a_mesh_made_in_code_is_reported_dropped() {
    let prefab = crate_prefab();
    let id = AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(id, prefab.record.clone());
    let (mut world, _, lid, _) = level_with_made_lid(prefab.member, id, &library);
    let overridden = AssetRef::<Mesh>::Asset(AssetUUID::new());
    assert!(world.set_component(lid, overridden), "the lid is remeshed");
    let lid_id = world.persistent_id(lid).expect("an id");
    let mut scene =
        khora_data::scene::collapse(&capture_world(&world).expect("captures"), &library)
            .expect("the scene collapses");
    let delta = &mut scene.instances[0].delta.changes;
    forge_made(delta, lid_id);

    let (expanded, report) =
        khora_data::scene::expand_reporting(&scene, &library).expect("the scene expands");
    let mut loaded = World::new();
    let applied = apply(&mut loaded, &expanded, Identity::Keep).expect("the scene loads");

    let lid = loaded.entity_with_id(lid_id).expect("the lid loads");
    assert_ne!(
        loaded
            .get::<AssetRef<Mesh>>(lid)
            .map(|mesh| matches!(mesh, AssetRef::<Mesh>::Made(_))),
        Some(true),
        "the lid kept a mesh made in code read from a file"
    );
    let reported = report.iter().chain(&applied.report.entries).any(|entry| {
        entry.entity == Some(lid_id)
            && entry.component.as_deref() == Some("AssetRef<Mesh>")
            && entry.kind == ReportKind::Dropped
    });
    assert!(
        reported,
        "the load does not report the override it refused: {report:?} {:?}",
        applied.report.entries
    );
}
