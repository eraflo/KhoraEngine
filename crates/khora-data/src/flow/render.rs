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

//! `RenderFlow` — projects the ECS World into a [`RenderWorld`] for the
//! render lanes to consume from the [`LaneBus`](khora_core::lane::LaneBus).
//!
//! This replaces the previous `extract_scene` free function called from the
//! engine tick. The Flow trait gives us a uniform pattern (select / adapt /
//! project) and AGDF-ready hooks for future per-domain adaptation (LOD,
//! frustum culling, etc.).

use khora_core::{
    math::{Mat4, Vec3},
    renderer::{api::scene::GpuMesh, light::LightType},
    ServiceRegistry,
};

use crate::ecs::{
    Camera, GlobalTransform, HandleComponent, Light, MaterialComponent, SemanticDomain, World,
};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::render::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

/// Projects the ECS World into the per-frame [`RenderWorld`] consumed by the
/// render lanes.
#[derive(Default)]
pub struct RenderFlow;

impl Flow for RenderFlow {
    type View = RenderWorld;

    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "render";

    fn project(&self, world: &World, _sel: &Selection, services: &ServiceRegistry) -> Self::View {
        let mut rw = RenderWorld::new();
        extract_meshes(world, &mut rw);
        extract_lights(world, &mut rw);
        extract_views(world, &mut rw);

        // No active scene Camera (e.g. editor in Editing mode where every
        // scene Camera is forced inactive)? Fall back to the shared
        // primary-view resolver, which consults `EditorViewportOverride`.
        if rw.views.is_empty() {
            if let Some(view) = crate::render::primary_view(world, services) {
                rw.views.push(view);
            }
        }
        rw
    }
}

register_flow!(RenderFlow);

fn extract_meshes(world: &World, render_world: &mut RenderWorld) {
    let query = world.query::<(&GlobalTransform, &HandleComponent<GpuMesh>)>();
    for (entity_id, (transform, gpu_mesh_handle)) in query.enumerate() {
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
            LightType::Directional(dir_light) => {
                global_transform.0.rotation() * dir_light.direction
            }
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

        render_world.views.push(ExtractedView {
            view_proj,
            position,
        });
    }
}
