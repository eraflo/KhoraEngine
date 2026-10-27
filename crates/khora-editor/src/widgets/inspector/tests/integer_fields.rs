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

//! **An integer field is a number to edit.** The inspector drags every number
//! as an `f32`; an edit of an integer field must still reach the component,
//! which reads that field as an integer.

use khora_sdk::khora_data::scene::registration_named;
use khora_sdk::prelude::ecs::AudioSource;
use khora_sdk::GameWorld;
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::walker::{render_value_marked, Marks};
use super::theme;

/// The label the walker gives the `priority` row.
const PRIORITY: &str = "Priority";

/// An audio source's priority, dragged to 200 in the inspector, is 200 on the
/// component.
#[test]
fn an_audio_source_priority_dragged_in_the_inspector_reaches_the_component() {
    let registration = registration_named("AudioSource").expect("AudioSource is registered");
    let mut world = GameWorld::new();
    let entity = world.spawn(AudioSource::default());
    let mut value = (registration.to_json)(world.inner_world(), entity)
        .expect("an audio source reads as the inspector's JSON");

    let mut ui = RecordingUiBuilder::new(420.0, 800.0).with_drag(PRIORITY, 200.0);
    let changed = render_value_marked(&mut ui, &mut value, &theme(), &mut Marks::default());
    assert!(
        changed,
        "the priority row is a number to drag: {:?}",
        ui.texts()
    );

    let written = (registration.from_json)(world.inner_world_mut(), entity, &value);
    assert!(
        written.is_ok(),
        "the edited source reads back: {written:?} from {value}"
    );
    assert_eq!(
        world
            .get_component::<AudioSource>(entity)
            .map(|s| s.priority),
        Some(200)
    );
}

/// A signed field holding a value at or above zero still goes below zero: a
/// UI element's z-index at 0, dragged to -3 in the inspector, is -3 on the
/// component.
#[test]
fn a_signed_integer_field_at_zero_is_dragged_below_zero() {
    let registration = registration_named("UiTransform").expect("UiTransform is registered");
    let mut world = GameWorld::new();
    let entity = world.spawn(khora_sdk::khora_data::UiTransform::default());
    let mut value = (registration.to_json)(world.inner_world(), entity)
        .expect("a UI transform reads as the inspector's JSON");

    let mut ui = RecordingUiBuilder::new(420.0, 800.0).with_drag("Z Index", -3.0);
    let changed = render_value_marked(&mut ui, &mut value, &theme(), &mut Marks::default());
    assert!(
        changed,
        "the z-index row is a number to drag: {:?}",
        ui.texts()
    );

    let written = (registration.from_json)(world.inner_world_mut(), entity, &value);
    assert!(
        written.is_ok(),
        "the edited transform reads back: {written:?} from {value}"
    );
    assert_eq!(
        world
            .get_component::<khora_sdk::khora_data::UiTransform>(entity)
            .map(|t| t.z_index),
        Some(-3),
        "the z-index the inspector wrote: {value}"
    );
}
