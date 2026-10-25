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

//! Scenes saved while a `Camera` still stored its aspect.
//!
//! A camera's aspect is the shape of the image it fills, read every frame
//! from the colour target; the camera no longer stores one. Scenes written
//! before that carry `aspect_ratio` on every camera, and `width` on every
//! orthographic projection. They load: the fields are dropped and the report
//! says so.

use khora_data::ecs::{Camera, ProjectionType, World};
use khora_data::scene::record::{Record, ReportEntry, ReportKind, VariantPayload};

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

/// The fields of the `Camera` record `record` holds for `id`, to add to.
fn camera_fields(record: &mut SceneRecord, id: PersistentId) -> &mut Vec<(String, Record)> {
    let value = value_mut(record, id, "Camera");
    match value {
        Record::Struct { fields, .. } => fields,
        other => panic!("a struct component is a struct record: {other:?}"),
    }
}

/// The report entry saying the `Camera` of `id` had `path` dropped.
fn dropped<'a>(
    entries: &'a [ReportEntry],
    id: PersistentId,
    path: &str,
) -> Option<&'a ReportEntry> {
    entries.iter().find(|entry| {
        entry.kind == ReportKind::Dropped
            && entry.entity == Some(id)
            && entry.component.as_deref() == Some("Camera")
            && entry.path == path
    })
}

/// A camera saved with the aspect it used to store loads as the camera it
/// is, and the report names `aspect_ratio` as dropped.
#[test]
fn a_camera_saved_with_an_aspect_ratio_loads_and_reports_it_dropped() {
    let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.05, 500.0);
    let (src, id) = world_with(camera);
    let mut record = capture_world(&src).expect("captures");
    camera_fields(&mut record, id).push(("aspect_ratio".into(), Record::F32(16.0 / 9.0)));

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a field the code dropped is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(dst.get::<Camera>(twin), Some(&camera), "{name}");
        assert!(
            dropped(&applied.report.entries, id, "aspect_ratio").is_some(),
            "{name}: `Camera.aspect_ratio` is not reported dropped: {:?}",
            applied.report
        );
    }
}

/// An orthographic camera saved with the width its projection used to
/// store loads with its height alone, and the report names
/// `projection.Orthographic.width` as dropped.
#[test]
fn an_orthographic_camera_saved_with_a_width_loads_and_reports_it_dropped() {
    let camera = Camera::new_orthographic(9.0, -1.0, 250.0);
    let (src, id) = world_with(camera);
    let mut record = capture_world(&src).expect("captures");
    let fields = camera_fields(&mut record, id);
    let (_, projection) = fields
        .iter_mut()
        .find(|(field, _)| field == "projection")
        .expect("the projection is recorded by name");
    match projection {
        Record::Variant {
            variant,
            payload: VariantPayload::Struct(payload),
            ..
        } if variant == "Orthographic" => {
            payload.push(("width".into(), Record::F32(16.0)));
        }
        other => panic!("an orthographic projection is a struct variant: {other:?}"),
    }
    fields.push(("aspect_ratio".into(), Record::F32(16.0 / 9.0)));

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a field the code dropped is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        let loaded = dst.get::<Camera>(twin).expect("the camera loaded");
        assert_eq!(loaded, &camera, "{name}");
        assert_eq!(
            loaded.projection,
            ProjectionType::Orthographic { height: 9.0 },
            "{name}"
        );
        for path in ["projection.Orthographic.width", "aspect_ratio"] {
            assert!(
                dropped(&applied.report.entries, id, path).is_some(),
                "{name}: `Camera.{path}` is not reported dropped: {:?}",
                applied.report
            );
        }
    }
}

/// A camera saved by today's code carries no aspect, so it loads with
/// nothing to report.
#[test]
fn a_camera_saved_today_loads_with_nothing_dropped() {
    for camera in [
        Camera::new_perspective(1.0, 0.1, 10.0),
        Camera::new_orthographic(4.0, 0.1, 10.0),
    ] {
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
                !applied
                    .report
                    .entries
                    .iter()
                    .any(|entry| entry.kind == ReportKind::Dropped),
                "{name}: {:?}",
                applied.report
            );
        }
    }
}

/// The old orthographic projection wrote `width` *before* `height`. A file
/// laid out that way still loads the height from `height`, not from the
/// first field of the payload.
#[test]
fn an_orthographic_camera_saved_width_first_loads_its_height_by_name() {
    let camera = Camera::new_orthographic(9.0, -1.0, 250.0);
    let (src, id) = world_with(camera);
    let mut record = capture_world(&src).expect("captures");
    let fields = camera_fields(&mut record, id);
    let (_, projection) = fields
        .iter_mut()
        .find(|(field, _)| field == "projection")
        .expect("the projection is recorded by name");
    match projection {
        Record::Variant {
            variant,
            payload: VariantPayload::Struct(payload),
            ..
        } if variant == "Orthographic" => {
            payload.insert(0, ("width".into(), Record::F32(16.0)));
        }
        other => panic!("an orthographic projection is a struct variant: {other:?}"),
    }
    fields.insert(1, ("aspect_ratio".into(), Record::F32(16.0 / 9.0)));

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a field the code dropped is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        let loaded = dst.get::<Camera>(twin).expect("the camera loaded");
        assert_eq!(
            loaded.projection,
            ProjectionType::Orthographic { height: 9.0 },
            "{name}"
        );
        assert_eq!(loaded, &camera, "{name}");
    }
}
