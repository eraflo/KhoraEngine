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
use khora_core::math::Mat4;
#[cfg(test)]
use khora_core::math::Vec3;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::GpuMaterial;
#[cfg(test)]
use khora_core::renderer::light::LightType;
use khora_core::Runtime;

#[cfg(test)]
use crate::ecs::Light;
use crate::ecs::{Camera, GlobalTransform, HandleComponent, SemanticDomain, World};
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
            // A resize reshapes every camera's projection without touching
            // the ECS.
            crate::render::target_aspect(runtime).to_bits() as u64,
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
        extract_views(world, &mut rw, crate::render::target_aspect(runtime));

        // No active scene Camera (e.g. editor in Editing mode where every
        // scene Camera is forced inactive)? Fall back to the shared
        // primary-view resolver, which consults `EditorViewportOverride` —
        // and gives nothing for an active camera whose values make no image,
        // so the override never stands in for it.
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

/// The lights that shine ([`shining_lights`](crate::render::shining_lights)),
/// in its order: position `i` here is light `i` of the shadow view too. A
/// light whose values make no light is not here — its problem is listed, its
/// values kept.
fn extract_lights(world: &World, render_world: &mut RenderWorld) {
    for shining in crate::render::shining_lights(world) {
        render_world.lights.push(ExtractedLight {
            light_type: shining.light.light_type,
            position: shining.position,
            direction: shining.direction,
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
    }
}

fn extract_views(world: &World, render_world: &mut RenderWorld, aspect: f32) {
    let camera_query = world.query::<(&Camera, &GlobalTransform)>();
    for (camera, global_transform) in camera_query {
        if !camera.is_active {
            continue;
        }

        let position = global_transform.0.translation();
        let view_matrix = crate::render::camera_view_matrix(global_transform);
        // A camera whose values make no image publishes no view; its
        // problem is reported by the component checks, not here. When it is
        // the first active camera — the one the frame renders from — no
        // other camera is drawn in its place.
        let Ok(projection) = camera.projection_matrix(aspect) else {
            if render_world.views.is_empty() {
                break;
            }
            continue;
        };
        let view_proj = projection * view_matrix;

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

    use crate::render::FALLBACK_ASPECT;

    /// A GPU-free run publishes no colour target, so the render view is
    /// projected at the fallback aspect — the same as `primary_view`.
    #[test]
    fn the_render_view_without_a_frame_context_uses_the_fallback_aspect() {
        let position = Vec3::new(2.0, 0.0, 4.0);
        let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.1, 50.0);
        let mut world = World::new();
        world.spawn((GlobalTransform::at_position(position), camera));

        let runtime = Runtime::default();
        let view = RenderFlow.project(&world, &Selection::new(), &runtime);
        assert_eq!(view.views.len(), 1);
        let expected = camera
            .projection_matrix(FALLBACK_ASPECT)
            .expect("a valid camera")
            * Mat4::from_translation(-position);
        for (a, e) in view.views[0]
            .view_proj
            .cols
            .iter()
            .zip(expected.cols.iter())
        {
            for (x, y) in [(a.x, e.x), (a.y, e.y), (a.z, e.z), (a.w, e.w)] {
                assert!(
                    (x - y).abs() < 1e-5,
                    "{:?} vs {expected:?}",
                    view.views[0].view_proj
                );
            }
        }
    }

    // --- A sheared pose seen the same way by every consumer -----------------

    use crate::flow::{ShadowFlow, ShadowMatrices};
    use khora_core::math::Vec4;

    fn assert_view_proj_close(actual: Mat4, expected: Mat4, what: &str) {
        for (a, e) in actual.cols.iter().zip(expected.cols.iter()) {
            for (x, y) in [(a.x, e.x), (a.y, e.y), (a.z, e.z), (a.w, e.w)] {
                assert!(
                    (x - y).abs() < 1e-5,
                    "{what}: {actual:?} where {expected:?} was expected"
                );
            }
        }
    }

    /// A camera under a stretched parent (a sheared world basis) is rendered
    /// through the view the flows slice their data around: `RenderFlow`'s
    /// view-projection is `primary_view`'s, and both are the camera's
    /// projection times the `camera_view_matrix` of its entity.
    #[test]
    fn a_sheared_camera_renders_the_view_the_flows_slice_around() {
        let global = GlobalTransform(light_under_a_stretched_parent());
        let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.1, 50.0);
        let mut world = World::new();
        world.spawn((global, camera));
        let runtime = Runtime::default();

        let rendered = RenderFlow.project(&world, &Selection::new(), &runtime);
        let primary = crate::render::primary_view(&world, &runtime).expect("an active camera");

        assert_eq!(rendered.views.len(), 1, "the one camera is extracted");
        assert_view_proj_close(
            rendered.views[0].view_proj,
            primary.view_proj,
            "rendered view against primary_view",
        );
        assert_view_proj_close(
            rendered.views[0].view_proj,
            camera
                .projection_matrix(FALLBACK_ASPECT)
                .expect("a valid camera")
                * crate::render::camera_view_matrix(&global),
            "rendered view against camera_view_matrix",
        );
    }

    /// A shadow-casting light under a stretched parent casts its shadow along
    /// the direction it is lit with: a spot light's shadow view looks down the
    /// extracted direction from the light's position, and a directional
    /// light's orthographic view sends that direction to its depth axis
    /// alone.
    #[test]
    fn a_shadowed_light_under_a_stretched_parent_casts_along_its_render_direction() {
        let pose = light_under_a_stretched_parent();
        let local = Vec3::new(0.3, -1.0, 0.2).normalize();
        let lights = [
            LightType::Spot(SpotLight {
                direction: local,
                shadow_enabled: true,
                ..SpotLight::default()
            }),
            LightType::Directional(DirectionalLight {
                direction: local,
                shadow_enabled: true,
                ..DirectionalLight::default()
            }),
        ];
        for light_type in lights {
            let is_spot = matches!(light_type, LightType::Spot(_));
            let mut world = World::new();
            world.spawn((GlobalTransform(pose), Light::new(light_type)));
            world.spawn((
                GlobalTransform::at_position(Vec3::new(0.0, 2.0, 10.0)),
                Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.1, 50.0),
            ));

            let rendered = extracted_light_direction(&world);
            let shadow = ShadowFlow.project(&world, &Selection::new(), &Runtime::default());
            let Some(ShadowMatrices::Single(view_proj)) = shadow.matrices.get(&0) else {
                panic!("the light casts one shadow matrix: {:?}", shadow.matrices);
            };

            if is_spot {
                let position = pose.translation();
                for distance in [1.0f32, 5.0] {
                    let clip = *view_proj * Vec4::from_vec3(position + rendered * distance, 1.0);
                    let (x, y) = (clip.x / clip.w, clip.y / clip.w);
                    assert!(
                        x.abs() < 1e-4 && y.abs() < 1e-4,
                        "a point {distance} along the spot's rendered direction lands at \
                         ({x}, {y}), off its shadow view's axis"
                    );
                }
            } else {
                let along = *view_proj * Vec4::from_vec3(rendered * 10.0, 0.0);
                assert!(
                    along.x.abs() < 1e-5 && along.y.abs() < 1e-5,
                    "the directional light's rendered direction moves across its shadow \
                     view: {along:?}"
                );
            }
        }
    }

    // --- A broken camera renders nothing ------------------------------------

    use crate::render::EditorViewportOverride;

    /// A world holding one active camera whose near plane sits at its eye,
    /// beside a drawable mesh and a light.
    fn world_with_a_broken_camera() -> World {
        let mut world = World::new();
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 1.0, 5.0)),
            Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.0, 50.0),
        ));
        spawn_drawable(&mut world, scaled(Vec3::ONE));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 5.0, 0.0)),
            Light::new(LightType::Directional(DirectionalLight::default())),
        ));
        world
    }

    /// A broken active camera: the flow runs to the end without panicking,
    /// still projects the scene, and publishes no view to render it from.
    #[test]
    fn render_flow_publishes_no_view_for_a_broken_camera() {
        let world = world_with_a_broken_camera();

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        assert!(
            view.views.is_empty(),
            "a broken camera published {} view(s)",
            view.views.len()
        );
        assert_eq!(view.meshes.len(), 1, "the scene is still projected");
        assert_eq!(view.lights.len(), 1, "the scene is still projected");
    }

    /// The editor's override is the view for *no* active camera; a broken
    /// active camera does not fall back to it.
    #[test]
    fn render_flow_publishes_no_view_for_a_broken_camera_even_with_an_editor_override() {
        let world = world_with_a_broken_camera();
        let editor = EditorViewportOverride::new();
        editor.set(Some(ExtractedView {
            view_proj: Mat4::from_translation(Vec3::new(4.0, 5.0, 6.0)),
            position: Vec3::new(-4.0, -5.0, -6.0),
        }));
        let mut runtime = Runtime::default();
        runtime.resources.insert(editor);

        let view = RenderFlow.project(&world, &Selection::new(), &runtime);

        assert!(
            view.views.is_empty(),
            "a broken camera fell back to the editor override: {:?}",
            view.views
        );
    }

    /// The first active camera is the one rendered from; when it is broken,
    /// the valid active camera after it is not rendered from in its place —
    /// the lit lanes read `views.first()`, `primary_view` (and so the shadow
    /// cascades) gives nothing.
    #[test]
    fn render_flow_renders_from_no_other_camera_when_the_first_active_one_is_broken() {
        let mut world = world_with_a_broken_camera();
        world.spawn((
            GlobalTransform::at_position(Vec3::new(-7.0, 0.0, 0.0)),
            Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.1, 50.0),
        ));
        assert!(
            crate::render::primary_view(&world, &Runtime::default()).is_none(),
            "the first active camera is the broken one"
        );

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        assert!(
            view.views.is_empty(),
            "the second active camera was rendered from in the broken one's place: {:?}",
            view.views
        );
    }

    // --- Only the lights that shine are extracted ---------------------------

    use khora_core::math::LinearRgba;
    use khora_core::renderer::light::PointLight;

    /// A world holding `lights`, in this order, each at its own place.
    fn world_of_lights(lights: &[(Light, Vec3)]) -> World {
        let mut world = World::new();
        for (light, position) in lights {
            world.spawn((GlobalTransform::at_position(*position), light.clone()));
        }
        world
    }

    /// A sun, a point light whose range is 0, and a spot: the broken point
    /// light is not extracted, and the sun and the spot are, in that order.
    #[test]
    fn a_light_that_does_not_shine_is_not_extracted() {
        let sun_at = Vec3::new(0.0, 10.0, 0.0);
        let spot_at = Vec3::new(3.0, 4.0, 0.0);
        let world = world_of_lights(&[
            (
                Light::new(LightType::Directional(DirectionalLight::default())),
                sun_at,
            ),
            (
                Light::new(LightType::Point(PointLight {
                    range: 0.0,
                    ..PointLight::default()
                })),
                Vec3::new(-3.0, 4.0, 0.0),
            ),
            (Light::new(LightType::Spot(SpotLight::default())), spot_at),
        ]);

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        let extracted: Vec<(&'static str, Vec3)> = view
            .lights
            .iter()
            .map(|light| {
                let kind = match light.light_type {
                    LightType::Directional(_) => "sun",
                    LightType::Point(_) => "point",
                    LightType::Spot(_) => "spot",
                };
                (kind, light.position)
            })
            .collect();
        assert_eq!(extracted, vec![("sun", sun_at), ("spot", spot_at)]);
    }

    fn vec_finite(v: Vec3) -> bool {
        v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
    }

    /// Swept over broken and usable lights, at usable and broken places:
    /// every field of every extracted light is finite — position, direction,
    /// shadow matrix, range, cones, intensity, colour — and a sun's or a
    /// spot's direction is a unit vector.
    #[test]
    fn every_extracted_light_value_is_finite() {
        let lights = [
            Light::new(LightType::Directional(DirectionalLight::default())),
            Light::new(LightType::Point(PointLight::default())),
            Light::new(LightType::Spot(SpotLight::default())),
            Light::new(LightType::Point(PointLight {
                range: f32::NAN,
                ..PointLight::default()
            })),
            Light::new(LightType::Point(PointLight {
                range: f32::INFINITY,
                ..PointLight::default()
            })),
            Light::new(LightType::Spot(SpotLight {
                outer_cone_angle: f32::NAN,
                ..SpotLight::default()
            })),
            Light::new(LightType::Spot(SpotLight {
                inner_cone_angle: f32::INFINITY,
                ..SpotLight::default()
            })),
            Light::new(LightType::Spot(SpotLight {
                direction: Vec3::ZERO,
                ..SpotLight::default()
            })),
            Light::new(LightType::Directional(DirectionalLight {
                direction: Vec3::ZERO,
                ..DirectionalLight::default()
            })),
            Light::new(LightType::Directional(DirectionalLight {
                direction: Vec3::new(1e-30, 0.0, 0.0),
                ..DirectionalLight::default()
            })),
            Light::new(LightType::Point(PointLight {
                intensity: f32::NAN,
                ..PointLight::default()
            })),
            Light::new(LightType::Directional(DirectionalLight {
                intensity: f32::INFINITY,
                ..DirectionalLight::default()
            })),
            Light::new(LightType::Spot(SpotLight {
                color: LinearRgba::new(1.0, f32::NAN, 1.0, 1.0),
                ..SpotLight::default()
            })),
        ];
        let places = [
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
        ];
        let mut entries = Vec::new();
        for (i, light) in lights.iter().enumerate() {
            for (j, place) in places.iter().enumerate() {
                // Each light at its own spot, so nothing overlaps.
                let offset = Vec3::new(i as f32, j as f32, 0.0);
                entries.push((light.clone(), *place + offset));
            }
        }
        let world = world_of_lights(&entries);

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        assert!(!view.lights.is_empty(), "the usable lights are extracted");
        let mut broken = Vec::new();
        for light in &view.lights {
            let (intensity, color, mut values) = match light.light_type {
                LightType::Directional(d) => (d.intensity, d.color, vec![]),
                LightType::Point(p) => (p.intensity, p.color, vec![p.range]),
                LightType::Spot(s) => (
                    s.intensity,
                    s.color,
                    vec![s.range, s.inner_cone_angle, s.outer_cone_angle],
                ),
            };
            values.extend([intensity, color.r, color.g, color.b]);
            let matrix_finite =
                light.shadow_view_proj.cols.iter().all(|c| {
                    c.x.is_finite() && c.y.is_finite() && c.z.is_finite() && c.w.is_finite()
                });
            let direction_ok = match light.light_type {
                LightType::Point(_) => vec_finite(light.direction),
                _ => vec_finite(light.direction) && (light.direction.length() - 1.0).abs() < 1e-5,
            };
            if !(vec_finite(light.position)
                && direction_ok
                && matrix_finite
                && values.iter().all(|v| v.is_finite()))
            {
                broken.push(format!("{light:?}"));
            }
        }
        assert!(
            broken.is_empty(),
            "extracted lights with a non-finite value or a non-unit direction:\n{}",
            broken.join("\n")
        );
    }

    /// `field` holds an `Error` in `light`'s own check.
    fn has_error_on(light: &Light, field: &str) -> bool {
        light
            .check()
            .iter()
            .any(|p| p.field == field && p.severity == khora_core::problems::ProblemSeverity::Error)
    }

    /// The shadow biases of a light that shines, shadow-casting or not, are
    /// finite: they reach the shader's shadow lookup as they are (a NaN
    /// normal bias sends the sample position, and so the shadow, nowhere).
    /// A light with a non-finite bias has its problem listed on that field,
    /// and is not extracted.
    #[test]
    fn an_extracted_light_has_finite_shadow_biases() {
        let mut lights = Vec::new();
        for bias in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            lights.push((
                "light_type.Directional.shadow_bias",
                Light::new(LightType::Directional(DirectionalLight {
                    shadow_enabled: true,
                    shadow_bias: bias,
                    ..DirectionalLight::default()
                })),
            ));
            lights.push((
                "light_type.Point.shadow_normal_bias",
                Light::new(LightType::Point(PointLight {
                    shadow_enabled: true,
                    shadow_normal_bias: bias,
                    ..PointLight::default()
                })),
            ));
            lights.push((
                "light_type.Spot.shadow_bias",
                Light::new(LightType::Spot(SpotLight {
                    shadow_enabled: true,
                    shadow_bias: bias,
                    ..SpotLight::default()
                })),
            ));
            lights.push((
                "light_type.Spot.shadow_normal_bias",
                Light::new(LightType::Spot(SpotLight {
                    shadow_normal_bias: bias,
                    ..SpotLight::default()
                })),
            ));
        }
        let unreported: Vec<String> = lights
            .iter()
            .filter(|(field, light)| !has_error_on(light, field))
            .map(|(field, light)| format!("no error on `{field}`: {:?}", light.check()))
            .collect();
        assert!(unreported.is_empty(), "{}", unreported.join("\n"));

        let entries: Vec<(Light, Vec3)> = lights
            .into_iter()
            .enumerate()
            .map(|(i, (_, light))| (light, Vec3::new(i as f32, 0.0, 0.0)))
            .collect();
        let world = world_of_lights(&entries);

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        let broken: Vec<String> = view
            .lights
            .iter()
            .filter(|light| {
                let (bias, normal_bias) = match light.light_type {
                    LightType::Directional(d) => (d.shadow_bias, d.shadow_normal_bias),
                    LightType::Point(p) => (p.shadow_bias, p.shadow_normal_bias),
                    LightType::Spot(s) => (s.shadow_bias, s.shadow_normal_bias),
                };
                !(bias.is_finite() && normal_bias.is_finite())
            })
            .map(|light| format!("{:?}", light.light_type))
            .collect();
        assert!(
            broken.is_empty(),
            "extracted lights with a non-finite shadow bias:\n{}",
            broken.join("\n")
        );
        assert!(
            view.lights.is_empty(),
            "none of them shines: {:?}",
            view.lights
        );
    }

    /// The radiance a light that shines sends — its colour times its
    /// intensity, the first product every lit shader takes — is finite. A
    /// light whose finite intensity and finite colour multiply past the
    /// largest float has its problem listed on its intensity, and is not
    /// extracted; a bright light whose product stays finite still is.
    #[test]
    fn an_extracted_light_has_a_finite_radiance() {
        let bright = LinearRgba::new(2.0, 2.0, 2.0, 1.0);
        let overflowing = [
            (
                "light_type.Directional.intensity",
                Light::new(LightType::Directional(DirectionalLight {
                    intensity: f32::MAX,
                    color: bright,
                    ..DirectionalLight::default()
                })),
            ),
            (
                "light_type.Point.intensity",
                Light::new(LightType::Point(PointLight {
                    intensity: 1e38,
                    color: LinearRgba::new(10.0, 1.0, 1.0, 1.0),
                    ..PointLight::default()
                })),
            ),
            (
                "light_type.Spot.intensity",
                Light::new(LightType::Spot(SpotLight {
                    intensity: f32::MAX,
                    color: bright,
                    ..SpotLight::default()
                })),
            ),
        ];
        let unreported: Vec<String> = overflowing
            .iter()
            .filter(|(field, light)| !has_error_on(light, field))
            .map(|(field, light)| format!("no error on `{field}`: {:?}", light.check()))
            .collect();
        assert!(unreported.is_empty(), "{}", unreported.join("\n"));

        // Bright, yet every channel's product stays finite.
        let bright_but_finite = Light::new(LightType::Point(PointLight {
            intensity: 1e30,
            color: bright,
            ..PointLight::default()
        }));
        let mut entries: Vec<(Light, Vec3)> = overflowing
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, (_, light))| (light, Vec3::new(i as f32, 0.0, 0.0)))
            .collect();
        entries.push((bright_but_finite, Vec3::new(9.0, 0.0, 0.0)));
        let world = world_of_lights(&entries);

        let view = RenderFlow.project(&world, &Selection::new(), &Runtime::default());

        let broken: Vec<String> = view
            .lights
            .iter()
            .filter(|light| {
                let (intensity, color) = match light.light_type {
                    LightType::Directional(d) => (d.intensity, d.color),
                    LightType::Point(p) => (p.intensity, p.color),
                    LightType::Spot(s) => (s.intensity, s.color),
                };
                ![color.r, color.g, color.b]
                    .iter()
                    .all(|c| (c * intensity).is_finite())
            })
            .map(|light| format!("{:?}", light.light_type))
            .collect();
        assert!(
            broken.is_empty(),
            "extracted lights whose colour × intensity overflows:\n{}",
            broken.join("\n")
        );
        assert_eq!(
            view.lights.len(),
            1,
            "only the bright light whose radiance stays finite shines: {:?}",
            view.lights
        );
        assert_eq!(view.lights[0].position, Vec3::new(9.0, 0.0, 0.0));
    }
}
