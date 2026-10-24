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

use khora_core::asset::Material;
use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::light::LightType;
use khora_core::Runtime;

use crate::ecs::{Camera, GlobalTransform, HandleComponent, Light, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::render::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};
use khora_core::ecs::entity::EntityId;
use khora_core::interpolation::{SharedTransformInterpolation, TransformInterpolation};

/// Projects the ECS World into the per-frame [`RenderWorld`] consumed by the
/// render lanes.
#[derive(Default)]
pub struct RenderFlow;

impl Flow for RenderFlow {
    type View = RenderWorld;

    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "render";

    /// The projection below reads mesh/material handles, `Light` and
    /// `Camera` (Render domain), `GlobalTransform` (Spatial domain), and —
    /// through `primary_view` — the editor viewport override, which is
    /// runtime state with no ECS epoch. Folding its bit-level fingerprint
    /// into the key keeps editor camera motion from serving stale views.
    fn cache_key(&self, world: &World, runtime: &Runtime) -> Option<u64> {
        Some(crate::flow::combine_cache_key([
            world.instance_id(),
            world.domain_epoch(SemanticDomain::Render),
            world.domain_epoch(SemanticDomain::Spatial),
            crate::render::editor_override_fingerprint(runtime),
            // Render interpolation makes the projected transforms a function of
            // the per-frame alpha, which moves without any ECS epoch change.
            // Fold its bit pattern in so the cached view is never served stale
            // while bodies visibly interpolate between two sim steps.
            render_interpolation_alpha(runtime).to_bits() as u64,
        ]))
    }

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let mut rw = RenderWorld::new();
        // Render-only interpolation factor between the previous and current
        // simulation step (0 when no decoupled sim / no Time resource).
        let alpha = render_interpolation_alpha(runtime);
        // Engine-owned previous-pose store (resource, not a component): held
        // read-locked across the mesh extraction. Absent in GPU-free tests.
        let interp = runtime.resources.get::<SharedTransformInterpolation>();
        let interp_guard = interp.as_ref().and_then(|shared| shared.read().ok());
        extract_meshes(world, &mut rw, alpha, interp_guard.as_deref());
        extract_lights(world, &mut rw);
        extract_views(world, &mut rw);

        // No active scene Camera (e.g. editor in Editing mode where every
        // scene Camera is forced inactive)? Fall back to the shared
        // primary-view resolver, which consults `EditorViewportOverride`.
        if rw.views.is_empty() {
            if let Some(view) = crate::render::primary_view(world, runtime) {
                rw.views.push(view);
            }
        }
        rw
    }
}

register_flow!(RenderFlow);

/// Reads the render-interpolation factor published by the scheduler into the
/// per-frame [`Time`](khora_core::time::Time) resource. Returns `0.0` (no
/// blend — render at the current transform) when the simulation isn't
/// decoupled or the resource is absent (e.g. GPU-free Flow tests).
fn render_interpolation_alpha(runtime: &Runtime) -> f32 {
    runtime
        .resources
        .get::<khora_core::time::SharedTime>()
        .and_then(|shared| shared.read().ok().map(|t| t.interpolation_alpha))
        .unwrap_or(0.0)
}

fn extract_meshes(
    world: &World,
    render_world: &mut RenderWorld,
    alpha: f32,
    interpolation: Option<&TransformInterpolation>,
) {
    let query = world.query::<(EntityId, &GlobalTransform, &HandleComponent<GpuMesh>)>();
    for (entity_id, transform, gpu_mesh_handle) in query {
        // Per-entity component lookup, NOT positional: entities with vs without
        // a material live in different archetypes, so correlating two separate
        // queries by enumerate-index would mismatch (and drop the GpuMaterial).
        let material = world
            .get::<HandleComponent<Box<dyn Material>>>(entity_id)
            .map(|m| m.handle.clone());
        let gpu_material = world
            .get::<HandleComponent<GpuMaterial>>(entity_id)
            .map(|h| h.handle.clone());

        // Render-only interpolation: when the engine recorded a previous
        // world-space pose for this entity (it is sim-moved), blend prev →
        // current by `alpha`. The authoritative `GlobalTransform` is untouched
        // — only the projected transform is interpolated ("adapt the HOW, not
        // the WHAT").
        let transform = match interpolation.and_then(|store| store.previous(entity_id)) {
            Some(prev) => prev.interpolate(&transform.0, alpha),
            None => transform.0,
        };

        // A negative determinant turns the mesh's faces inside out: the lit
        // lanes cull its front faces instead of its back ones.
        let [right, up, forward] = [0, 1, 2].map(|i| transform.0.cols[i].truncate());
        let mirrored = right.dot(up.cross(forward)) < 0.0;

        render_world.meshes.push(ExtractedMesh {
            transform,
            cpu_mesh_uuid: gpu_mesh_handle.uuid,
            gpu_mesh: gpu_mesh_handle.handle.clone(),
            material,
            gpu_material,
            mirrored,
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
                global_transform.0.transform_direction(dir_light.direction)
            }
            LightType::Spot(spot_light) => {
                global_transform.0.transform_direction(spot_light.direction)
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::math::affine_transform::AffineTransform;
    use khora_core::math::{Mat3, Quaternion};
    use khora_core::renderer::api::pipeline::PrimitiveTopology;
    use khora_core::renderer::api::resource::{BufferId, IndexFormat};

    /// Translation ∘ rotation ∘ scale, the order `Transform::to_mat4` composes.
    fn trs(translation: Vec3, rotation: Quaternion, scale: Vec3) -> AffineTransform {
        AffineTransform(
            Mat4::from_translation(translation)
                * Mat4::from_quat(rotation)
                * Mat4::from_scale(scale),
        )
    }

    fn scaled(scale: Vec3) -> AffineTransform {
        trs(Vec3::ZERO, Quaternion::IDENTITY, scale)
    }

    /// Spawns a drawable entity at `pose` — its global transform plus an
    /// already-uploaded mesh — and returns the entity and the mesh uuid that
    /// identifies its extracted draw.
    fn spawn_drawable(world: &mut World, pose: AffineTransform) -> (EntityId, AssetUUID) {
        let uuid = AssetUUID::new();
        let mesh = HandleComponent {
            handle: AssetHandle::new(GpuMesh {
                vertex_buffer: BufferId(0),
                index_buffer: BufferId(0),
                index_count: 0,
                index_format: IndexFormat::Uint32,
                primitive_topology: PrimitiveTopology::TriangleList,
            }),
            uuid,
        };
        let entity = world.spawn((GlobalTransform(pose), mesh));
        (entity, uuid)
    }

    /// The draw extracted for the mesh `uuid`.
    fn draw_of(render_world: &RenderWorld, uuid: AssetUUID) -> &ExtractedMesh {
        render_world
            .meshes
            .iter()
            .find(|mesh| mesh.cpu_mesh_uuid == uuid)
            .expect("every drawable entity is extracted")
    }

    fn extract(
        world: &World,
        alpha: f32,
        interpolation: Option<&TransformInterpolation>,
    ) -> RenderWorld {
        let mut render_world = RenderWorld::new();
        extract_meshes(world, &mut render_world, alpha, interpolation);
        render_world
    }

    fn determinant(transform: &AffineTransform) -> f32 {
        Mat3::from_mat4(&transform.0).determinant()
    }

    /// One negative scale axis mirrors the mesh.
    #[test]
    fn one_negative_scale_axis_is_extracted_mirrored() {
        let mut world = World::new();
        let (_, mirrored_x) = spawn_drawable(&mut world, scaled(Vec3::new(-1.0, 1.0, 1.0)));
        let (_, mirrored_y) = spawn_drawable(&mut world, scaled(Vec3::new(2.0, -0.5, 3.0)));

        let render_world = extract(&world, 0.0, None);

        assert!(
            draw_of(&render_world, mirrored_x).mirrored,
            "-X is a mirror"
        );
        assert!(
            draw_of(&render_world, mirrored_y).mirrored,
            "-Y is a mirror"
        );
    }

    /// A mirror under a rotation is still a mirror: the decision reads the
    /// determinant of the drawn matrix, not the sign of a matrix element.
    #[test]
    fn a_rotated_mirror_is_extracted_mirrored() {
        let mut world = World::new();
        let pose = trs(
            Vec3::new(5.0, -1.0, 2.0),
            Quaternion::from_axis_angle(Vec3::new(0.2, 1.0, -0.4).normalize(), 2.4),
            Vec3::new(1.5, 1.5, -1.5),
        );
        let (_, uuid) = spawn_drawable(&mut world, pose);

        let render_world = extract(&world, 0.0, None);

        assert!(draw_of(&render_world, uuid).mirrored);
    }

    /// Three negative axes are a mirror composed with a half-turn: still a
    /// mirror.
    #[test]
    fn three_negative_scale_axes_are_extracted_mirrored() {
        let mut world = World::new();
        let (_, uuid) = spawn_drawable(&mut world, scaled(Vec3::new(-1.0, -2.0, -1.0)));

        let render_world = extract(&world, 0.0, None);

        assert!(draw_of(&render_world, uuid).mirrored);
    }

    /// Only an odd number of negative axes mirrors: none or two (a half-turn)
    /// keep the winding, beside a one-axis mirror extracted in the same pass.
    #[test]
    fn only_an_odd_number_of_negative_scale_axes_is_extracted_mirrored() {
        let mut world = World::new();
        let (_, plain) = spawn_drawable(&mut world, scaled(Vec3::new(1.0, 2.0, 0.5)));
        let (_, two_negative) = spawn_drawable(&mut world, scaled(Vec3::new(-1.0, -1.0, 1.0)));
        let (_, rotated_two_negative) = spawn_drawable(
            &mut world,
            trs(
                Vec3::ZERO,
                Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.7),
                Vec3::new(1.0, -3.0, -1.0),
            ),
        );
        let (_, one_negative) = spawn_drawable(&mut world, scaled(Vec3::new(1.0, 1.0, -1.0)));

        let render_world = extract(&world, 0.0, None);

        assert!(!draw_of(&render_world, plain).mirrored, "no negative axis");
        assert!(
            !draw_of(&render_world, two_negative).mirrored,
            "two negative axes"
        );
        assert!(
            !draw_of(&render_world, rotated_two_negative).mirrored,
            "two negative axes under a rotation"
        );
        assert!(
            draw_of(&render_world, one_negative).mirrored,
            "one negative axis"
        );
    }

    /// A mirrored entity that the simulation moved is drawn interpolated
    /// between its previous and current poses — both mirrored — and the drawn
    /// pose is still a mirror, and marked so.
    #[test]
    fn a_mirrored_entity_extracted_with_interpolation_stays_mirrored() {
        let mut world = World::new();
        let previous = trs(
            Vec3::ZERO,
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.3),
            Vec3::new(-1.0, 1.0, 1.0),
        );
        let current = trs(
            Vec3::new(10.0, 0.0, 0.0),
            Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 1.1),
            Vec3::new(-1.0, 1.0, 1.0),
        );
        let (entity, uuid) = spawn_drawable(&mut world, current);
        let mut interpolation = TransformInterpolation::new();
        interpolation.record(entity, previous);

        let render_world = extract(&world, 0.5, Some(&interpolation));
        let draw = draw_of(&render_world, uuid);

        let det = determinant(&draw.transform);
        assert!(
            det < 0.0,
            "the drawn pose lost its mirror (determinant {det})"
        );
        assert!(
            draw.mirrored,
            "the drawn pose is a mirror but is not marked so"
        );
    }

    use khora_core::renderer::light::{DirectionalLight, SpotLight};

    /// An obliquely turned light under a parent stretched to three times its
    /// height: a sheared world basis, whose linear part makes a direction
    /// lean away from where the rotation alone would turn it.
    fn light_under_a_stretched_parent() -> AffineTransform {
        AffineTransform(
            Mat4::from_scale(Vec3::new(1.0, 3.0, 1.0))
                * trs(
                    Vec3::new(2.0, 5.0, -1.0),
                    Quaternion::from_axis_angle(Vec3::new(1.0, 0.5, -0.3).normalize(), 0.9),
                    Vec3::ONE,
                )
                .0,
        )
    }

    /// The direction `RenderFlow` extracts for the one light in `world`.
    fn extracted_light_direction(world: &World) -> Vec3 {
        let view = RenderFlow.project(world, &Selection::new(), &Runtime::default());
        assert_eq!(view.lights.len(), 1, "the one light is extracted");
        view.lights[0].direction
    }

    fn assert_direction_close(actual: Vec3, expected: Vec3, what: &str) {
        let error = (actual - expected).length();
        assert!(
            error < 1e-5,
            "{what}: {actual:?} where {expected:?} was expected (off by {error})"
        );
    }

    /// A directional light on an entity under a stretched parent shines
    /// along its local direction carried by the entity's world basis —
    /// normalised, leaning as the stretched geometry around it leans — not
    /// along the rotation alone.
    #[test]
    fn a_directional_light_under_a_stretched_parent_shines_along_its_world_direction() {
        let pose = light_under_a_stretched_parent();
        let local = Vec3::new(0.3, -1.0, 0.2).normalize();
        let mut world = World::new();
        world.spawn((
            GlobalTransform(pose),
            Light::new(LightType::Directional(DirectionalLight {
                direction: local,
                ..DirectionalLight::default()
            })),
        ));

        assert_direction_close(
            extracted_light_direction(&world),
            pose.0.transform_vector(local).normalize(),
            "directional light under a (1, 3, 1) parent",
        );
    }

    /// A spot light under the same stretched parent points along its world
    /// direction too.
    #[test]
    fn a_spot_light_under_a_stretched_parent_points_along_its_world_direction() {
        let pose = light_under_a_stretched_parent();
        let local = Vec3::new(-0.4, -1.0, 0.6).normalize();
        let mut world = World::new();
        world.spawn((
            GlobalTransform(pose),
            Light::new(LightType::Spot(SpotLight {
                direction: local,
                ..SpotLight::default()
            })),
        ));

        assert_direction_close(
            extracted_light_direction(&world),
            pose.0.transform_vector(local).normalize(),
            "spot light under a (1, 3, 1) parent",
        );
    }
}
