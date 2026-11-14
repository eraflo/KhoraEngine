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

//! Spawning entities the editor asked for.

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

/// Returns the material a freshly spawned primitive gets: a neutral matte grey.
///
/// The engine has no implicit default material (a meshed entity without one is
/// a logged error), so the authoring tool supplies one.
fn default_surface_material() -> khora_sdk::prelude::materials::StandardMaterial {
    khora_sdk::prelude::materials::StandardMaterial {
        base_color: khora_sdk::prelude::math::LinearRgba::new(0.7, 0.7, 0.7, 1.0),
        roughness: 0.8,
        ..Default::default()
    }
}

/// Spawns an entity named `name` showing the mesh asset `uuid` at `point`.
///
/// A default surface material makes it render at once; the mesh loads from its
/// `MeshRef::Asset` on the next tick.
pub fn spawn_mesh_asset(
    world: &mut GameWorld,
    uuid: khora_sdk::khora_core::asset::AssetUUID,
    point: [f32; 3],
    name: &str,
) -> EntityId {
    let mat = world.add_material(default_surface_material());
    let entity = world.spawn((
        Transform::from_translation(khora_sdk::prelude::math::Vec3::new(
            point[0], point[1], point[2],
        )),
        GlobalTransform::identity(),
        Name::new(name),
        MeshRef::Asset(uuid),
    ));
    world.add_component(entity, mat);
    world.inner_world_mut().mark_authored(entity);
    entity
}

/// Processes pending spawn requests from the scene tree panel.
pub fn process_spawns(world: &mut GameWorld, state: &mut EditorState) {
    if let Some(request) = state.pending_spawn.take() {
        let entity = match request.as_str() {
            "Cube" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_cube_at(world, khora_sdk::prelude::math::Vec3::ZERO, 1.0)
                    .with_component(Name::new("Cube"))
                    .with_component(mat)
                    .build()
            }
            "Sphere" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_sphere(world, 0.5, 16, 16)
                    .with_component(Name::new("Sphere"))
                    .with_component(mat)
                    .build()
            }
            "Plane" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_plane(world, 10.0, 0.0)
                    .with_component(Name::new("Plane"))
                    .with_component(mat)
                    .build()
            }
            "Light" => world.spawn((
                Transform::identity(),
                GlobalTransform::identity(),
                Name::new("Light"),
                Light::directional(),
            )),
            "Camera" => {
                let cam = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 1000.0);
                world.spawn((
                    Transform::identity(),
                    GlobalTransform::identity(),
                    Name::new("Camera"),
                    cam,
                ))
            }
            _ => world.spawn((
                Transform::identity(),
                GlobalTransform::identity(),
                Name::new(&request),
            )),
        };

        // An author made it: it keeps the identity a scene saves it under.
        world.inner_world_mut().mark_authored(entity);
        state.select(entity);
        log::info!("Spawned entity {:?} ({})", entity, request);
    }
}
