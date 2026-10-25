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

//! **A non-finite float is a number in the inspector.** JSON has no `inf`,
//! so the inspector's JSON carries one as a `{"$float": "inf"}` marker; the
//! walker must show it as the number row it is, leave it alone until it is
//! edited, and write an edit back in a form `from_json` reads.

use khora_sdk::prelude::ecs::Camera;
use khora_tool_ui::testing::RecordingUiBuilder;
use serde_json::json;

use super::super::renderers::{f32_to_json, json_to_f32};
use super::super::walker::{render_value_marked, Marks};
use super::{camera_json, camera_registration, theme, world_with};

/// The label the walker gives the `z_far` row.
const Z_FAR: &str = "Z Far";

#[test]
fn a_non_finite_float_is_shown_as_a_number_and_stays_editable() {
    let camera = Camera {
        z_far: f32::INFINITY,
        ..Camera::default_perspective()
    };
    let (mut world, entity) = world_with(camera);
    let original = camera_json(&world, entity);
    assert_eq!(
        original["z_far"],
        json!({ "$float": "inf" }),
        "the inspector's JSON marks an infinite far plane"
    );
    let theme = theme();

    // Shown, untouched: a number row, not the card of an object or a variant.
    let mut value = original.clone();
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);
    let changed = render_value_marked(&mut ui, &mut value, &theme, &mut Marks::default());
    assert!(
        !ui.painted_text("$float"),
        "the marker is no object to unfold: {:?}",
        ui.texts()
    );
    assert!(!changed, "showing the value edits nothing");
    assert_eq!(value, original, "a value not edited is left as it is");

    // Edited to a finite value: a plain number, which the component reads.
    let mut ui = RecordingUiBuilder::new(420.0, 800.0).with_drag(Z_FAR, 250.0);
    let changed = render_value_marked(&mut ui, &mut value, &theme, &mut Marks::default());
    assert!(changed, "the z_far row is a number to drag");
    assert_eq!(value["z_far"].as_f64(), Some(250.0), "a plain number");
    (camera_registration().from_json)(world.inner_world_mut(), entity, &value)
        .expect("the edited camera reads back");
    let read = world
        .get_component::<Camera>(entity)
        .expect("the camera is still there");
    assert_eq!(read.z_far, 250.0);
    assert_eq!(read.z_near, camera.z_near, "the other fields are kept");
}

/// The number bridge reads each marker as the float it stands for — what
/// the number row is handed to show.
#[test]
fn a_float_marker_reads_as_its_number() {
    assert_eq!(
        json_to_f32(Some(&json!({ "$float": "inf" }))),
        f32::INFINITY
    );
    assert_eq!(
        json_to_f32(Some(&json!({ "$float": "-inf" }))),
        f32::NEG_INFINITY
    );
    assert!(json_to_f32(Some(&json!({ "$float": "nan" }))).is_nan());
    assert_eq!(
        json_to_f32(Some(&json!(2.5))),
        2.5,
        "a plain number as before"
    );
}

/// A row dragged or typed to infinity writes the marker, not `null`: the
/// edit reaches the component instead of being refused.
#[test]
fn a_number_row_set_to_infinity_writes_the_marker() {
    assert_eq!(f32_to_json(f32::INFINITY), json!({ "$float": "inf" }));
    assert_eq!(f32_to_json(f32::NEG_INFINITY), json!({ "$float": "-inf" }));
    assert_eq!(f32_to_json(f32::NAN), json!({ "$float": "nan" }));

    let camera = Camera::default_perspective();
    let (mut world, entity) = world_with(camera);
    let mut value = camera_json(&world, entity);
    let mut ui = RecordingUiBuilder::new(420.0, 800.0).with_drag(Z_FAR, f32::INFINITY);
    let changed = render_value_marked(&mut ui, &mut value, &theme(), &mut Marks::default());
    assert!(changed);
    (camera_registration().from_json)(world.inner_world_mut(), entity, &value)
        .expect("an infinite far plane is a value the camera holds");
    assert_eq!(
        world.get_component::<Camera>(entity).map(|c| c.z_far),
        Some(f32::INFINITY)
    );
}
