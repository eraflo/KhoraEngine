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

//! Free functions that populate the [`RenderWorld`] (and other render-data
//! views) from the ECS `World`.
//!
//! Per CLAD these are **not** registered as services: extraction is data-
//! layer logic invoked directly by the engine's hot loop, not by agents.

use khora_core::{
    math::{Mat4, Vec3},
    renderer::{api::resource::ViewInfo, api::scene::GpuMesh, light::LightType},
};

use crate::ecs::{Camera, GlobalTransform, HandleComponent, Light, MaterialComponent, World};

use super::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

/// Populates `render_world` from `world`.  The previous frame's data is
/// cleared first.  Idempotent: safe to call repeatedly per frame.
///
/// This is the canonical scene-extraction entry point.  Called by the
/// engine in `tick_with_services()` after the GPU mesh sync, **before** any
/// agent runs.
pub fn extract_scene(world: &World, render_world: &mut RenderWorld) {
    render_world.clear();
    extract_meshes(world, render_world);
    extract_lights(world, render_world);
    extract_views(world, render_world);
}

fn extract_meshes(world: &World, render_world: &mut RenderWorld) {
    let query = world.query::<(&GlobalTransform, &HandleComponent<GpuMesh>)>();
    for (entity_id, (transform, gpu_mesh_handle)) in query.enumerate() {
        // Optional material — looked up via a separate query because we don't
        // require it to participate in the main archetype filter.
        let material = world
            .query::<&MaterialComponent>()
            .nth(entity_id)
            .map(|m| m.handle.clone());

        render_world.meshes.push(ExtractedMesh {
            transform: transform.0,
            cpu_mesh_uuid: gpu_mesh_handle.uuid,
            gpu_mesh: gpu_mesh_handle.handle.clone(),
            material,
        });
    }
}

fn extract_lights(world: &World, render_world: &mut RenderWorld) {
    let light_query = world.query::<(&Light, &GlobalTransform)>();
    for (light_comp, global_transform) in light_query {
        if !light_comp.enabled {
            continue;
        }

        let position = global_transform.0.translation();
        let direction = match &light_comp.light_type {
            LightType::Directional(dir_light) => global_transform.0.rotation() * dir_light.direction,
            LightType::Spot(spot_light) => global_transform.0.rotation() * spot_light.direction,
            LightType::Point(_) => Vec3::ZERO,
        };

        render_world.lights.push(ExtractedLight {
            light_type: light_comp.light_type,
            position,
            direction,
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
    }
}

fn extract_views(world: &World, render_world: &mut RenderWorld) {
    let camera_query = world.query::<(&Camera, &GlobalTransform)>();
    for (camera, global_transform) in camera_query {
        if !camera.is_active {
            continue;
        }

        let position = global_transform.0.translation();
        let rotation = global_transform.0.rotation();

        let rotation_matrix = Mat4::from_quat(rotation.inverse());
        let translation_matrix = Mat4::from_translation(-position);
        let view_matrix = rotation_matrix * translation_matrix;
        let proj_matrix = camera.projection_matrix();
        let view_proj = proj_matrix * view_matrix;

        render_world.views.push(ExtractedView { view_proj, position });
    }
}

/// Extracts the first **active** camera as a [`ViewInfo`] suitable for pushing
/// to the renderer.
///
/// Returns `None` if no entity has an active camera (editor edit mode, empty
/// scene, etc.) — callers can fall back to a default `ViewInfo`.
pub fn extract_active_camera_view(world: &World) -> Option<ViewInfo> {
    for (camera, global_transform) in world.query::<(&Camera, &GlobalTransform)>() {
        if !camera.is_active {
            continue;
        }

        let world_matrix = global_transform.to_matrix();
        let camera_position = Vec3::new(
            world_matrix.cols[3][0],
            world_matrix.cols[3][1],
            world_matrix.cols[3][2],
        );
        let view_matrix = world_matrix.inverse().unwrap_or(Mat4::IDENTITY);
        let projection_matrix = camera.projection_matrix();

        return Some(ViewInfo::new(view_matrix, projection_matrix, camera_position));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::Transform;
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::math::{affine_transform::AffineTransform, Quaternion};

    fn dummy_gpu_mesh() -> GpuMesh {
        use khora_core::renderer::api::{
            pipeline::enums::PrimitiveTopology, resource::BufferId, util::IndexFormat,
        };
        GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(0),
            index_count: 0,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }
    }

    #[test]
    fn extract_scene_empty_world_yields_empty_render_world() {
        let world = World::new();
        let mut rw = RenderWorld::default();
        extract_scene(&world, &mut rw);
        assert!(rw.meshes.is_empty());
        assert!(rw.lights.is_empty());
        assert!(rw.views.is_empty());
    }

    #[test]
    fn extract_scene_populates_meshes() {
        let mut world = World::new();
        let transform =
            GlobalTransform(AffineTransform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
        let uuid = AssetUUID::new();
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(dummy_gpu_mesh()),
                uuid,
            },
        ));

        let mut rw = RenderWorld::default();
        extract_scene(&world, &mut rw);
        assert_eq!(rw.meshes.len(), 1);
        assert_eq!(rw.meshes[0].cpu_mesh_uuid, uuid);
        assert_eq!(rw.meshes[0].transform.translation(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn extract_scene_clears_previous_frame() {
        let mut world = World::new();
        world.spawn((
            GlobalTransform(AffineTransform::IDENTITY),
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        let mut rw = RenderWorld::default();
        extract_scene(&world, &mut rw);
        assert_eq!(rw.meshes.len(), 1);

        let world2 = World::new();
        extract_scene(&world2, &mut rw);
        assert!(rw.meshes.is_empty());
    }

    #[test]
    fn extract_active_camera_view_returns_none_when_no_camera() {
        let world = World::new();
        assert!(extract_active_camera_view(&world).is_none());
    }

    #[test]
    fn extract_active_camera_view_skips_inactive_cameras() {
        let mut world = World::new();
        let camera = Camera {
            is_active: false,
            ..Default::default()
        };
        let transform = Transform::default();
        world.spawn((camera, transform, GlobalTransform::new(transform.to_mat4())));
        assert!(extract_active_camera_view(&world).is_none());
    }

    #[test]
    fn extract_active_camera_view_picks_first_active_camera() {
        let mut world = World::new();

        let inactive = Camera {
            is_active: false,
            ..Default::default()
        };
        let active = Camera::new_perspective(90.0_f32.to_radians(), 1.0, 1.0, 100.0);

        let t1 = Transform::new(Vec3::new(10.0, 0.0, 0.0), Quaternion::IDENTITY, Vec3::ONE);
        let t2 = Transform::new(Vec3::new(0.0, 10.0, 0.0), Quaternion::IDENTITY, Vec3::ONE);

        world.spawn((inactive, t1, GlobalTransform::new(t1.to_mat4())));
        world.spawn((active, t2, GlobalTransform::new(t2.to_mat4())));

        let view = extract_active_camera_view(&world).expect("active camera present");
        assert_eq!(view.camera_position, Vec3::new(0.0, 10.0, 0.0));
        assert_ne!(view.view_matrix, Mat4::IDENTITY);
    }
}
