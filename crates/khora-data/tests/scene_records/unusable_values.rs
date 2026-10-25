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

//! A scene holding a value the engine cannot use loads it as written and
//! says so.
//!
//! A camera whose near plane sits at its eye cannot be projected, yet the
//! value is the author's: the load keeps it, and the report names the field
//! with a [`ReportKind::Unusable`] entry, so the tool can show it. A snapshot
//! is a copy of a live world the running checks already see: it reports
//! nothing.

use khora_core::problems::ProblemSeverity;
use khora_data::ecs::{Camera, FieldProblem, World};
use khora_data::scene::record::{unusable_entries, ReportEntry, ReportKind};

use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// A world holding `camera` on one authored entity, and that entity's
/// identity.
fn world_with(camera: Camera) -> (World, PersistentId) {
    let mut world = World::new();
    let entity = world.spawn(camera);
    world.mark_authored(entity).expect("authored");
    let id = world.persistent_id(entity).expect("id");
    (world, id)
}

/// The `Unusable` entries of `entries`.
fn unusable(entries: &[ReportEntry]) -> Vec<&ReportEntry> {
    entries
        .iter()
        .filter(|entry| matches!(entry.kind, ReportKind::Unusable { .. }))
        .collect()
}

/// A camera saved with its near plane at 0 loads, in every encoding, with
/// its near plane still at 0 — and the report holds one `Unusable` entry on
/// `z_near`, naming the entity and the component, its message naming the
/// value.
#[test]
fn a_scene_with_an_unusable_camera_loads_and_says_so() {
    let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.0, 500.0);
    let (src, id) = world_with(camera);
    let record = capture_world(&src).expect("captures");

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: an unusable value is no load error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        let loaded = dst.get::<Camera>(twin).expect("the camera loaded");
        assert_eq!(
            loaded.z_near.to_bits(),
            0.0f32.to_bits(),
            "{name}: near kept"
        );
        assert_eq!(loaded, &camera, "{name}: the camera as written");

        let entries = unusable(&applied.report.entries);
        assert_eq!(
            entries.len(),
            1,
            "{name}: one unusable field expected: {:?}",
            applied.report
        );
        let entry = entries[0];
        assert_eq!(entry.entity, Some(id), "{name}");
        assert_eq!(entry.component.as_deref(), Some("Camera"), "{name}");
        assert_eq!(entry.path, "z_near", "{name}");
        match &entry.kind {
            ReportKind::Unusable { message } => {
                assert!(message.contains('0'), "{name}: {message}")
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

/// An orthographic camera's near plane may sit behind its eye: a negative
/// near is usable, and the load reports nothing unusable.
#[test]
fn an_orthographic_camera_with_a_negative_near_loads_clean() {
    let camera = Camera::new_orthographic(9.0, -1.0, 250.0);
    let (src, id) = world_with(camera);
    let record = capture_world(&src).expect("captures");

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(dst.get::<Camera>(twin), Some(&camera), "{name}");
        assert!(
            unusable(&applied.report.entries).is_empty(),
            "{name}: {:?}",
            applied.report
        );
    }
}

/// A broken camera through a snapshot and back keeps its value, and the
/// snapshot reports nothing: it copies a live world, not authored data.
#[test]
fn a_snapshot_restore_adds_no_unusable_entries() {
    let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.0, 500.0);
    let (src, id) = world_with(camera);
    let file = snapshot_of(&src);

    let mut dst = World::new();
    let applied =
        load_snapshot(&mut dst, &file).unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));
    let twin = dst.entity_with_id(id).expect("restored");
    assert_eq!(dst.get::<Camera>(twin), Some(&camera), "the value kept");
    assert!(
        applied.report.is_clean(),
        "a snapshot reported something: {:?}",
        applied.report.entries
    );
}

/// Only an `Error` makes a value unusable: each becomes one `Unusable`
/// entry, its path the field, its message the problem's; a `Warning` is no
/// entry.
#[test]
fn unusable_entries_turn_only_errors_into_entries() {
    let entries = unusable_entries(vec![
        FieldProblem {
            field: "z_near",
            severity: ProblemSeverity::Error,
            message: "near must be greater than 0 (it is 0)".to_owned(),
        },
        FieldProblem {
            field: "light_type.Spot.range",
            severity: ProblemSeverity::Warning,
            message: "casts no shadow: range must exceed 0.1".to_owned(),
        },
        FieldProblem {
            field: "z_far",
            severity: ProblemSeverity::Error,
            message: "far must be greater than near (near 0, far 0)".to_owned(),
        },
    ]);

    assert_eq!(
        entries,
        vec![
            ReportEntry {
                entity: None,
                component: None,
                path: "z_near".to_owned(),
                kind: ReportKind::Unusable {
                    message: "near must be greater than 0 (it is 0)".to_owned(),
                },
            },
            ReportEntry {
                entity: None,
                component: None,
                path: "z_far".to_owned(),
                kind: ReportKind::Unusable {
                    message: "far must be greater than near (near 0, far 0)".to_owned(),
                },
            },
        ]
    );
}

/// No problem, no entry.
#[test]
fn unusable_entries_of_no_problem_is_empty() {
    assert!(unusable_entries(Vec::new()).is_empty());
}

/// A prefab whose member `Eye` holds a camera whose near plane is 0, the
/// library holding it, and the member's id in the prefab.
fn rig_prefab() -> (
    khora_core::asset::AssetUUID,
    super::prefab_sample::Library,
    PersistentId,
) {
    use khora_data::ecs::Name;
    use khora_data::scene::capture_subtree;

    let mut prefab_world = World::new();
    let root = prefab_world.spawn(Name::new("Rig"));
    let eye = prefab_world.spawn((Name::new("Eye"), broken_rig_camera()));
    prefab_world.set_parent(eye, Some(root));
    for entity in [root, eye] {
        prefab_world.mark_authored(entity).expect("authored");
    }
    let eye_id = prefab_world.persistent_id(eye).expect("id");
    let prefab = khora_core::asset::AssetUUID::new();
    let library = super::prefab_sample::Library::default().with(
        prefab,
        capture_subtree(&prefab_world, root).expect("captures"),
    );
    (prefab, library, eye_id)
}

/// The rig prefab's camera: its near plane at 0.
fn broken_rig_camera() -> Camera {
    Camera::new_perspective(1.0, 0.0, 100.0)
}

/// A scene holding one instance of the rig prefab, its member camera given
/// `camera` when there is one, loaded through expansion: the loaded world,
/// the member's id in it, and every report entry of the load.
fn load_rig_instance(camera: Option<Camera>) -> (World, PersistentId, Vec<ReportEntry>) {
    use khora_data::scene::{expand_reporting, instantiate_prefab};

    use super::prefab_sample::save;

    let (prefab, library, eye_id) = rig_prefab();
    let mut scene = World::new();
    let instance = instantiate_prefab(&mut scene, prefab, &library).expect("instantiates");
    let instance_id = scene.persistent_id(instance).expect("id");
    let member = PersistentId::within(instance_id, eye_id);
    if let Some(camera) = camera {
        let eye = scene.entity_with_id(member).expect("the member");
        *scene.get_mut::<Camera>(eye).expect("a camera") = camera;
    }
    let record = save(&scene, &library);

    let (expanded, left_out) = expand_reporting(&record, &library).expect("expands");
    let mut dst = World::new();
    let applied = apply(&mut dst, &expanded, Identity::Keep).expect("loads");
    let mut entries = applied.report.entries.clone();
    entries.extend(left_out);
    (dst, member, entries)
}

/// A prefab member's broken camera, loaded through an instance, is reported
/// once, on the member's id under the instance.
#[test]
fn a_prefab_member_with_an_unusable_camera_is_reported_once_on_its_instance_id() {
    let (dst, member, entries) = load_rig_instance(None);

    let twin = dst.entity_with_id(member).expect("the member loaded");
    assert_eq!(dst.get::<Camera>(twin), Some(&broken_rig_camera()));
    let found = unusable(&entries);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].entity, Some(member));
    assert_eq!(found[0].component.as_deref(), Some("Camera"));
    assert_eq!(found[0].path, "z_near");
}

/// An instance that moved its member camera's near plane to 0.1 loads a
/// usable camera, and the load reports nothing unusable: the prefab's 0 is
/// not what loads.
#[test]
fn an_instance_fixing_its_prefab_camera_reports_nothing_unusable() {
    let fixed = Camera::new_perspective(1.0, 0.1, 100.0);
    let (dst, member, entries) = load_rig_instance(Some(fixed));

    let twin = dst.entity_with_id(member).expect("the member loaded");
    assert_eq!(dst.get::<Camera>(twin), Some(&fixed));
    let found = unusable(&entries);
    assert!(found.is_empty(), "{found:?}");
}

/// A camera whose far plane is infinite takes an edit of another field
/// through its JSON — what the inspector writes back — and keeps its far
/// plane: an unusable value is neither refused nor rewritten by an edit
/// beside it.
#[test]
fn a_camera_with_an_infinite_far_takes_an_edit_through_its_json() {
    let camera = Camera::new_perspective(1.0, 0.1, f32::INFINITY);
    let mut world = World::new();
    let entity = world.spawn(camera);
    let registration = inventory::iter::<ComponentRegistration>
        .into_iter()
        .find(|registration| registration.type_name == "Camera")
        .expect("the camera is registered");

    let mut json = (registration.to_json)(&world, entity).expect("the camera's JSON");
    json["is_active"] = serde_json::Value::Bool(false);
    let edited = (registration.from_json)(&mut world, entity, &json);

    assert_eq!(edited, Ok(()), "the edit was refused; the JSON was {json}");
    let back = world.get::<Camera>(entity).expect("still a camera");
    assert!(!back.is_active, "the edit landed");
    assert_eq!(back.z_far, f32::INFINITY, "the far plane is kept");
}
