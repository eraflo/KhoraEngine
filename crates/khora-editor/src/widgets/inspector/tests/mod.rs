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

//! The inspector's JSON walker against a recorder: a field the component's
//! check finds unusable says so under its row, and a value the JSON cannot
//! hold as a number — an infinite far plane — is still a number to read and
//! edit.

mod field_problems;
mod integer_fields;
mod non_finite_floats;

use khora_sdk::editor_ui::UiTheme;
use khora_sdk::khora_data::scene::{registration_named, ComponentRegistration};
use khora_sdk::prelude::ecs::{Camera, EntityId, Name};
use khora_sdk::GameWorld;
use serde_json::Value;

/// The theme the editor paints with.
fn theme() -> UiTheme {
    khora_tool_ui::brand::khora_dark()
}

/// The camera's registration — what the inspector reads and writes it by.
fn camera_registration() -> &'static ComponentRegistration {
    registration_named("Camera").expect("Camera is a registered component")
}

/// A world holding `camera`, named "Lens", and the entity holding it.
fn world_with(camera: Camera) -> (GameWorld, EntityId) {
    let mut world = GameWorld::new();
    let entity = world.spawn((camera, Name::new("Lens")));
    (world, entity)
}

/// The camera of `entity` as the inspector receives it.
fn camera_json(world: &GameWorld, entity: EntityId) -> Value {
    (camera_registration().to_json)(world.inner_world(), entity)
        .expect("a camera reads as the inspector's JSON")
}
