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

//! `ShadowFlow` — derives, per shadow-casting light, the view-projection
//! matrix the shadow pass will use to render its depth slice.
//!
//! Lives in the Substrate Pass: pure data derivation, no GPU work. The
//! [`ShadowPassLane`] reads the resulting [`ShadowView`] from the
//! [`LaneBus`](khora_core::lane::LaneBus) and only handles atlas allocation
//! and depth rendering — the math has moved to where it belongs (Data).
//!
//! # Index alignment
//!
//! Light indices in [`ShadowView::matrices`] match light positions in
//! `RenderWorld.lights`: both are the positions in one list,
//! [`shining_lights`](crate::render::shining_lights). The shadow lane and the
//! lit lanes therefore agree on which `i` refers to which light.

use std::collections::HashMap;

use khora_core::math::{Mat4, Vec3, Vec4};
use khora_core::renderer::light::LightType;
use khora_core::Runtime;

#[cfg(test)]
use crate::ecs::{GlobalTransform, Light};
use crate::ecs::{SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::render::{primary_view, ExtractedView};

/// View-projection matrices a single shadow-casting light publishes.
///
/// Two variants reflect the two atlas binding surfaces consumed by the lit
/// shader: directional / spot use a single matrix sampled against the 2D
/// depth-array atlas, point lights use six matrices sampled against the
/// cubemap atlas.
///
/// `Cube` is significantly larger than `Single` (6 × `Mat4` ≈ 384 B vs
/// `Mat4` ≈ 64 B). The variants are boxed to even out memory usage, since
/// most lights are directional / spot and a fat enum would inflate the
/// per-frame `HashMap<usize, ShadowMatrices>`.
#[derive(Debug, Clone)]
pub enum ShadowMatrices {
    /// One view-projection — directional or spot light.
    Single(Mat4),
    /// Six view-projections in [`khora_core::math::CubeFace::ALL`] order
    /// (`[+X, -X, +Y, -Y, +Z, -Z]`) — point light.
    Cube(Box<[Mat4; 6]>),
}

/// Output of [`ShadowFlow`].
#[derive(Debug, Default, Clone)]
pub struct ShadowView {
    /// Number of lights that shine this frame — the length of
    /// [`shining_lights`](crate::render::shining_lights) — whether they cast
    /// a shadow or not.
    pub light_count: usize,
    /// Per-light shadow data, keyed by the light's position in
    /// `RenderWorld.lights`. Only shadow-casting lights have an entry.
    pub matrices: HashMap<usize, ShadowMatrices>,
}

/// Computes shadow view-projection matrices.
#[derive(Default)]
pub struct ShadowFlow;

impl Flow for ShadowFlow {
    type View = ShadowView;

    // No dedicated shadow domain — bucketed under Render for budget purposes.
    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "shadow";

    /// Same inputs as `RenderFlow`: `Light` / `Camera` (Render domain),
    /// `GlobalTransform` (Spatial domain), `primary_view`'s editor viewport
    /// override (runtime state, fingerprinted bit-for-bit), and the target
    /// aspect the camera is projected at (cascades slice that frustum).
    fn cache_key(&self, world: &World, runtime: &Runtime) -> Option<u64> {
        Some(crate::flow::combine_cache_key([
            world.instance_id(),
            world.domain_epoch(SemanticDomain::Render),
            world.domain_epoch(SemanticDomain::Spatial),
            crate::render::editor_override_fingerprint(runtime),
            crate::render::target_aspect(runtime).to_bits() as u64,
        ]))
    }

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let camera_view = primary_view(world, runtime);
        let mut matrices: HashMap<usize, ShadowMatrices> = HashMap::new();
        let mut light_count = 0;

        // The list RenderFlow reads too: indices align by construction.
        for shining in crate::render::shining_lights(world) {
            let light = shining.light;
            let light_index = light_count;
            light_count += 1;

            let casts_shadow = match &light.light_type {
                LightType::Directional(d) => d.shadow_enabled,
                LightType::Point(p) => p.shadow_enabled,
                LightType::Spot(s) => s.shadow_enabled,
            };
            if !casts_shadow {
                continue;
            }

            let position = shining.position;

            match &light.light_type {
                LightType::Point(p) => {
                    // Six view-proj matrices, one per cube face. The math
                    // (90° FOV, near `SHADOW_NEAR`, far = light range, wgpu
                    // cubemap basis) lives in `Mat4::try_cube_face_view_projs`.
                    // A range that makes no cube casts no shadow; the light's
                    // own check reports why.
                    let Some(faces) = Mat4::try_cube_face_view_projs(position, p.range) else {
                        continue;
                    };
                    matrices.insert(light_index, ShadowMatrices::Cube(Box::new(faces)));
                }
                LightType::Directional(_) | LightType::Spot(_) => {
                    // Single matrix — needs the camera frustum for CSM /
                    // perspective-spot derivations.
                    let Some(camera) = camera_view.as_ref() else {
                        continue;
                    };
                    // A light that shines points somewhere (a unit
                    // direction); a cone or range that makes no projection
                    // casts no shadow, and the light's own check says why.
                    let direction = shining.direction;
                    let Some(view_proj) = compute_single_shadow_view_proj(
                        &light.light_type,
                        position,
                        direction,
                        camera,
                    ) else {
                        continue;
                    };
                    // A cone narrow enough projects, yet its product with the
                    // light's view can still overflow.
                    if !is_finite(&view_proj) {
                        continue;
                    }
                    matrices.insert(light_index, ShadowMatrices::Single(view_proj));
                }
            }
        }

        ShadowView {
            light_count,
            matrices,
        }
    }
}

register_flow!(ShadowFlow);

// ─── pure shadow-matrix math (moved out of `ShadowPassLane`) ─────────

/// Whether every entry of `m` is finite.
fn is_finite(m: &Mat4) -> bool {
    m.cols
        .iter()
        .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite() && c.w.is_finite())
}

fn compute_single_shadow_view_proj(
    light_type: &LightType,
    position: Vec3,
    direction: Vec3,
    camera: &ExtractedView,
) -> Option<Mat4> {
    match light_type {
        LightType::Directional(_) => Some(directional_shadow_view_proj(direction, camera)),
        LightType::Spot(sl) => {
            let up = if direction.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let view = Mat4::look_at_rh(position, position + direction, up)?;
            let proj = Mat4::try_perspective_rh_zo(
                sl.outer_cone_angle * 2.0,
                1.0,
                Mat4::SHADOW_NEAR,
                sl.range,
            )?;
            Some(proj * view)
        }
        // Point lights are routed through `Mat4::try_cube_face_view_projs`
        // directly in `project()`; they never reach this single-matrix path.
        LightType::Point(_) => unreachable!(
            "point lights are handled by Mat4::try_cube_face_view_projs in ShadowFlow::project"
        ),
    }
}

/// World-space distance the directional shadow cascade covers, measured
/// from the camera near plane along each frustum edge.
///
/// A directional light has no position, so the shadow map must be fitted
/// to a *bounded* slice of the camera frustum: fitting the full near→far
/// range (cameras commonly use a 1000-unit far plane) spreads the 2048²
/// atlas across ~2000 world units — roughly one world unit per texel —
/// which is far too coarse to resolve sub-unit casters, so their shadows
/// vanish entirely. Capping the cascade concentrates the texels near the
/// viewer where shadows read; casters beyond this distance are covered by
/// the ortho z-padding but not finely shadowed. This is a shadow-quality
/// (representation) constant, not a scene-semantics value.
const SHADOW_CASCADE_DISTANCE: f32 = 60.0;

/// CSM (cascaded shadow map) view-projection for a directional light.
fn directional_shadow_view_proj(direction: Vec3, camera: &ExtractedView) -> Mat4 {
    // 1. Camera frustum corners in world space. Each (x, y) edge yields a
    // near (z=0) and far (z=1) corner; the far corner is pulled back along
    // the near→far ray so the cascade covers at most
    // `SHADOW_CASCADE_DISTANCE` world units rather than the full far plane.
    let inv_view_proj = camera.view_proj.inverse().unwrap_or(Mat4::IDENTITY);
    let mut corners = Vec::with_capacity(8);
    for x in &[-1.0_f32, 1.0] {
        for y in &[-1.0_f32, 1.0] {
            let near_h = inv_view_proj * Vec4::new(*x, *y, 0.0, 1.0);
            let far_h = inv_view_proj * Vec4::new(*x, *y, 1.0, 1.0);
            let near = near_h.truncate() / near_h.w;
            let far = far_h.truncate() / far_h.w;

            let edge = far - near;
            let edge_len = edge.length();
            let clamped_far = if edge_len > SHADOW_CASCADE_DISTANCE {
                near + edge * (SHADOW_CASCADE_DISTANCE / edge_len)
            } else {
                far
            };

            corners.push(near);
            corners.push(clamped_far);
        }
    }

    // 2. Light view matrix centered on frustum center.
    let light_dir = direction.normalize();
    let up = if light_dir.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };

    let mut center = Vec3::ZERO;
    for p in &corners {
        center = center + *p;
    }
    center = center / 8.0;

    let light_view = Mat4::look_at_rh(center, center + light_dir, up).unwrap_or(Mat4::IDENTITY);

    // 3. Frustum AABB in light space.
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for p in corners {
        let p_ls = light_view * Vec4::from_vec3(p, 1.0);
        min.x = min.x.min(p_ls.x);
        max.x = max.x.max(p_ls.x);
        min.y = min.y.min(p_ls.y);
        max.y = max.y.max(p_ls.y);
        min.z = min.z.min(p_ls.z);
        max.z = max.z.max(p_ls.z);
    }

    // 4. Texel snapping to prevent shimmer when the camera moves.
    let shadow_map_size = 2048.0_f32;
    let units_per_texel_x = (max.x - min.x) / shadow_map_size;
    let units_per_texel_y = (max.y - min.y) / shadow_map_size;
    min.x = (min.x / units_per_texel_x).floor() * units_per_texel_x;
    max.x = (max.x / units_per_texel_x).floor() * units_per_texel_x;
    min.y = (min.y / units_per_texel_y).floor() * units_per_texel_y;
    max.y = (max.y / units_per_texel_y).floor() * units_per_texel_y;

    // 5. Ortho projection — z padding for casters outside the frustum.
    let z_padding = 100.0;
    let light_proj = Mat4::orthographic_rh_zo(
        min.x,
        max.x,
        min.y,
        max.y,
        min.z - z_padding,
        max.z + z_padding,
    );

    light_proj * light_view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::Camera;
    use crate::render::EditorViewportOverride;
    use khora_core::renderer::light::{DirectionalLight, PointLight};

    /// A world holding, in this order, a shadow-casting directional light
    /// (index 0), a shadow-casting point light (index 1), and one active
    /// camera whose near plane sits at its eye.
    fn lights_and_a_broken_camera() -> World {
        let mut world = World::new();
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 10.0, 0.0)),
            Light::new(LightType::Directional(DirectionalLight {
                direction: Vec3::new(0.3, -1.0, 0.2).normalize(),
                shadow_enabled: true,
                ..DirectionalLight::default()
            })),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(2.0, 3.0, -1.0)),
            Light::new(LightType::Point(PointLight {
                range: 10.0,
                shadow_enabled: true,
                ..PointLight::default()
            })),
        ));
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 2.0, 10.0)),
            Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.0, 50.0),
        ));
        world
    }

    /// A broken active camera gives the directional light no frustum to fit
    /// its shadow to: the flow runs without panicking, the directional light
    /// gets no matrix, and the point light — which needs no camera — keeps
    /// its six, at its own index.
    #[test]
    fn shadow_flow_survives_a_broken_camera() {
        let world = lights_and_a_broken_camera();

        let shadow = ShadowFlow.project(&world, &Selection::new(), &Runtime::default());

        assert_eq!(shadow.light_count, 2, "both lights are counted");
        assert!(
            !shadow.matrices.contains_key(&0),
            "the directional light got a matrix from a broken camera: {:?}",
            shadow.matrices.get(&0)
        );
        assert!(
            matches!(shadow.matrices.get(&1), Some(ShadowMatrices::Cube(_))),
            "the point light keeps its cube matrices: {:?}",
            shadow.matrices.get(&1)
        );
    }

    /// The editor's override stands in only for *no* active camera: with a
    /// broken active camera, the directional light does not fit its shadow
    /// to the override's view.
    #[test]
    fn shadow_flow_with_a_broken_camera_ignores_the_editor_override() {
        let world = lights_and_a_broken_camera();
        let editor = EditorViewportOverride::new();
        editor.set(Some(ExtractedView {
            view_proj: Mat4::perspective_rh_zo(std::f32::consts::FRAC_PI_3, 1.0, 0.1, 50.0)
                * Mat4::from_translation(Vec3::new(0.0, -2.0, -10.0)),
            position: Vec3::new(0.0, 2.0, 10.0),
        }));
        let mut runtime = Runtime::default();
        runtime.resources.insert(editor);

        let shadow = ShadowFlow.project(&world, &Selection::new(), &runtime);

        assert!(
            !shadow.matrices.contains_key(&0),
            "the directional light fitted its shadow to the editor override: {:?}",
            shadow.matrices.get(&0)
        );
    }

    // --- A light whose shadow cannot be built casts none --------------------

    use khora_core::renderer::light::SpotLight;

    /// A shadow-casting spot light of `range`.
    fn shadowed_spot(range: f32) -> Light {
        Light::new(LightType::Spot(SpotLight {
            range,
            shadow_enabled: true,
            ..SpotLight::default()
        }))
    }

    /// A shadow-casting point light of `range`.
    fn shadowed_point(range: f32) -> Light {
        Light::new(LightType::Point(PointLight {
            range,
            shadow_enabled: true,
            ..PointLight::default()
        }))
    }

    /// A shadow-casting sun.
    fn shadowed_sun() -> Light {
        Light::new(LightType::Directional(DirectionalLight {
            direction: Vec3::new(0.3, -1.0, 0.2).normalize(),
            shadow_enabled: true,
            ..DirectionalLight::default()
        }))
    }

    /// A world holding `lights`, in this order, each at its own place, and a
    /// usable active camera the sun and the spots fit their shadows to.
    fn lit_world(lights: &[(Light, Vec3)]) -> World {
        let mut world = World::new();
        for (light, position) in lights {
            world.spawn((GlobalTransform::at_position(*position), light.clone()));
        }
        world.spawn((
            GlobalTransform::at_position(Vec3::new(0.0, 2.0, 10.0)),
            Camera::default_perspective(),
        ));
        world
    }

    /// The matrices a light published, as a list (one for a sun or a spot,
    /// six for a point light).
    fn published(view: &ShadowView, index: usize) -> Option<Vec<Mat4>> {
        view.matrices.get(&index).map(|matrices| match matrices {
            ShadowMatrices::Single(m) => vec![*m],
            ShadowMatrices::Cube(faces) => faces.to_vec(),
        })
    }

    /// Projects `lights` with and without the light at `tiny`, and checks the
    /// tiny light casts no shadow while every other light publishes exactly
    /// what it publishes when the tiny one is gone.
    ///
    /// Where the others sit depends on the tiny light's check:
    /// - only Warnings (or nothing): the tiny light still shines — it is
    ///   counted, gets no matrix at its own index, and every other light
    ///   keeps its own index;
    /// - an Error: the tiny light does not shine — it is not counted, and
    ///   every light after it moves down one index, to its place among the
    ///   lights that shine.
    fn assert_tiny_light_casts_no_shadow(lights: Vec<(Light, Vec3)>, tiny: usize) {
        use khora_core::problems::ProblemSeverity;

        let shines = !lights[tiny]
            .0
            .check()
            .iter()
            .any(|p| p.severity == ProblemSeverity::Error);
        let world = lit_world(&lights);
        let with = ShadowFlow.project(&world, &Selection::new(), &Runtime::default());
        let mut others = lights.clone();
        others.remove(tiny);
        let without =
            ShadowFlow.project(&lit_world(&others), &Selection::new(), &Runtime::default());

        let counted = if shines {
            lights.len()
        } else {
            lights.len() - 1
        };
        assert_eq!(
            with.light_count, counted,
            "every light that shines is counted, and only those (the tiny light shines: {shines})"
        );
        assert_eq!(
            shining_lights(&world).len(),
            counted,
            "the shadow count is the count of the lights that shine"
        );
        if shines {
            assert!(
                !with.matrices.contains_key(&tiny),
                "the tiny light got a shadow matrix: {:?}",
                with.matrices.get(&tiny)
            );
        }
        for index in with.matrices.keys() {
            assert!(
                *index < with.light_count,
                "a matrix at index {index}, past the {} lights that shine",
                with.light_count
            );
        }
        assert_eq!(with.matrices.len(), lights.len() - 1, "{:?}", with.matrices);
        for index in 0..lights.len() {
            if index == tiny {
                continue;
            }
            let alone = if index < tiny { index } else { index - 1 };
            let at = if shines { index } else { alone };
            let own = published(&with, at);
            assert!(own.is_some(), "light {index} (at {at}) lost its shadow");
            assert_eq!(
                own,
                published(&without, alone),
                "light {index} (at {at}) publishes what it publishes without the tiny light"
            );
        }
    }

    /// A shadow-casting spot light whose range (0.05) does not reach past
    /// the shadow near plane: the flow runs without panicking, the spot gets
    /// no matrix, and the sun, the other spot and the point light keep
    /// theirs at their own indices.
    #[test]
    fn a_tiny_spot_light_casts_no_shadow_and_does_not_panic() {
        assert_tiny_light_casts_no_shadow(
            vec![
                (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0)),
                (shadowed_spot(0.05), Vec3::new(-3.0, 4.0, 0.0)),
                (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0)),
                (shadowed_point(10.0), Vec3::new(2.0, 3.0, -1.0)),
            ],
            1,
        );
    }

    /// The same for a shadow-casting point light of range 0.05: no cube
    /// faces for it, no panic, the others untouched.
    #[test]
    fn a_tiny_point_light_casts_no_shadow_and_does_not_panic() {
        assert_tiny_light_casts_no_shadow(
            vec![
                (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0)),
                (shadowed_point(0.05), Vec3::new(-3.0, 4.0, 0.0)),
                (shadowed_point(10.0), Vec3::new(2.0, 3.0, -1.0)),
                (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0)),
            ],
            1,
        );
    }

    /// Ranges a shadow cannot be built from give no matrix and no panic, for
    /// spot and point lights alike. Exactly the shadow near plane is only a
    /// Warning: the light still shines and is counted. Zero, negative, not a
    /// number or infinite is an Error: the light does not shine, is not
    /// counted, and the light after it moves down to its index.
    #[test]
    fn a_light_with_an_unusable_range_casts_no_shadow() {
        for range in [0.1, 0.0, -2.0, f32::NAN, f32::INFINITY] {
            for light in [shadowed_spot(range), shadowed_point(range)] {
                assert_tiny_light_casts_no_shadow(
                    vec![
                        (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0)),
                        (light, Vec3::new(-3.0, 4.0, 0.0)),
                        (shadowed_point(10.0), Vec3::new(2.0, 3.0, -1.0)),
                    ],
                    1,
                );
            }
        }
    }

    /// A spot cone of 90° (a 180° field of view) cannot be held by a
    /// perspective: an Error, so the spot does not shine — it is not counted
    /// and casts no shadow rather than a garbage matrix; the spot after it
    /// moves down to its index.
    #[test]
    fn a_spot_light_with_a_half_space_cone_casts_no_shadow() {
        let wide = Light::new(LightType::Spot(SpotLight {
            outer_cone_angle: std::f32::consts::FRAC_PI_2,
            shadow_enabled: true,
            ..SpotLight::default()
        }));
        assert_tiny_light_casts_no_shadow(
            vec![
                (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0)),
                (wide, Vec3::new(-3.0, 4.0, 0.0)),
                (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0)),
            ],
            1,
        );
    }
    /// Every matrix a light publishes is finite — a narrow but usable spot
    /// cone (1e-38 rad) projected from a light off the origin overflows the
    /// product of its projection and its view.
    #[test]
    fn a_published_shadow_matrix_is_finite() {
        let narrow = Light::new(LightType::Spot(SpotLight {
            direction: Vec3::new(0.0, 0.0, -1.0),
            inner_cone_angle: 0.0,
            outer_cone_angle: 1e-38,
            shadow_enabled: true,
            ..SpotLight::default()
        }));
        assert!(narrow.check().is_empty(), "{:?}", narrow.check());

        let view = ShadowFlow.project(
            &lit_world(&[(narrow, Vec3::new(1.0, 4.0, 2.0))]),
            &Selection::new(),
            &Runtime::default(),
        );

        for m in published(&view, 0).unwrap_or_default() {
            let finite = m
                .cols
                .iter()
                .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite() && c.w.is_finite());
            assert!(finite, "a non-finite shadow matrix was published: {m:?}");
        }
    }

    /// A shadow-casting point or spot light whose check is clean (no error,
    /// no warning) gets its shadow, and one whose check is not gets none —
    /// swept over ranges and cone angles, subnormal cones included.
    #[test]
    fn a_light_casts_a_shadow_exactly_when_its_check_is_clean() {
        use khora_core::problems::ProblemSeverity;
        let ranges = [
            -1.0,
            0.0,
            0.05,
            0.1,
            0.100_000_01,
            0.2,
            10.0,
            1e30,
            f32::MAX,
        ];
        let cones = [
            0.0,
            1e-45,
            1e-39,
            1e-30,
            1e-4,
            0.6,
            1.5,
            std::f32::consts::FRAC_PI_2,
            f32::NAN,
        ];
        let mut lights = Vec::new();
        for range in ranges {
            lights.push(shadowed_point(range));
            for outer in cones {
                lights.push(Light::new(LightType::Spot(SpotLight {
                    range,
                    inner_cone_angle: 0.0,
                    outer_cone_angle: outer,
                    shadow_enabled: true,
                    ..SpotLight::default()
                })));
            }
        }

        let mut disagreements = Vec::new();
        for light in lights {
            let clean = light.check().iter().all(|p| {
                p.severity != ProblemSeverity::Error && p.severity != ProblemSeverity::Warning
            });
            let view = ShadowFlow.project(
                &lit_world(&[(light.clone(), Vec3::new(-3.0, 4.0, 0.0))]),
                &Selection::new(),
                &Runtime::default(),
            );
            if clean != view.matrices.contains_key(&0) {
                disagreements.push(format!(
                    "check clean: {clean}, shadow: {} — {:?}",
                    view.matrices.contains_key(&0),
                    light.light_type
                ));
            }
        }
        assert!(disagreements.is_empty(), "{}", disagreements.join("\n"));
    }

    /// A sun or a spot whose direction is zero or not a number points
    /// nowhere — an Error, so it does not shine: it is not counted and casts
    /// no shadow rather than one rendered through an arbitrary view; the
    /// light after it moves down to its index.
    #[test]
    fn a_light_with_no_direction_casts_no_shadow() {
        for direction in [Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0)] {
            let sun = Light::new(LightType::Directional(DirectionalLight {
                direction,
                shadow_enabled: true,
                ..DirectionalLight::default()
            }));
            let spot = Light::new(LightType::Spot(SpotLight {
                direction,
                shadow_enabled: true,
                ..SpotLight::default()
            }));
            for light in [sun, spot] {
                assert_tiny_light_casts_no_shadow(
                    vec![
                        (shadowed_point(10.0), Vec3::new(2.0, 3.0, -1.0)),
                        (light, Vec3::new(-3.0, 4.0, 0.0)),
                        (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0)),
                    ],
                    1,
                );
            }
        }
    }

    // --- Shadow indices are the indices of the lights that shine ------------

    use crate::flow::RenderFlow;
    use crate::render::shining_lights;

    /// A shadow-casting sun, a shadow-casting point light whose range is 0
    /// (it does not shine), and a shadow-casting spot: two lights are
    /// counted, and the spot's matrix sits at index 1 — the one it publishes
    /// when the broken light is not there at all.
    #[test]
    fn shadow_indices_follow_the_lights_that_shine() {
        let sun = (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0));
        let spot = (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0));
        let broken = (shadowed_point(0.0), Vec3::new(-3.0, 4.0, 0.0));

        let with = ShadowFlow.project(
            &lit_world(&[sun.clone(), broken, spot.clone()]),
            &Selection::new(),
            &Runtime::default(),
        );
        let without = ShadowFlow.project(
            &lit_world(&[sun, spot]),
            &Selection::new(),
            &Runtime::default(),
        );

        assert_eq!(
            with.light_count, 2,
            "only the lights that shine are counted"
        );
        assert!(
            !with.matrices.contains_key(&2),
            "a matrix sits past the lights that shine: {:?}",
            with.matrices.keys().collect::<Vec<_>>()
        );
        let spot_matrix = published(&with, 1);
        assert!(
            spot_matrix.is_some(),
            "the spot's matrix is at index 1: {:?}",
            with.matrices.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            spot_matrix,
            published(&without, 1),
            "the spot publishes what it publishes without the broken light"
        );
        assert_eq!(published(&with, 0), published(&without, 0), "the sun's");
    }

    /// Over a world mixing usable, broken, disabled and misplaced lights, the
    /// render extraction, the shadow derivation and the list of lights that
    /// shine count the same lights, and the matrix at every shadow index is
    /// the one the light at that index of the list publishes alone.
    #[test]
    fn render_and_shadow_agree_on_every_light_index() {
        let mut disabled_spot = shadowed_spot(15.0);
        disabled_spot.enabled = false;
        let pointless_sun = Light::new(LightType::Directional(DirectionalLight {
            direction: Vec3::ZERO,
            shadow_enabled: true,
            ..DirectionalLight::default()
        }));
        let lights = vec![
            (shadowed_point(f32::NAN), Vec3::new(-6.0, 4.0, 0.0)),
            (shadowed_sun(), Vec3::new(0.0, 10.0, 0.0)),
            (shadowed_spot(0.0), Vec3::new(-3.0, 4.0, 0.0)),
            (shadowed_spot(15.0), Vec3::new(3.0, 4.0, 0.0)),
            (disabled_spot, Vec3::new(5.0, 4.0, 0.0)),
            (pointless_sun, Vec3::new(0.0, 12.0, 0.0)),
            (shadowed_point(10.0), Vec3::new(2.0, 3.0, -1.0)),
            (shadowed_spot(12.0), Vec3::new(f32::NAN, 4.0, 0.0)),
            (shadowed_spot(0.05), Vec3::new(-1.0, 4.0, 2.0)),
            (Light::point(), Vec3::new(1.0, 1.0, 1.0)),
            (shadowed_spot(8.0), Vec3::new(4.0, 2.0, -3.0)),
        ];
        let world = lit_world(&lights);
        let runtime = Runtime::default();

        let shining = shining_lights(&world);
        let rendered = RenderFlow.project(&world, &Selection::new(), &runtime);
        let shadow = ShadowFlow.project(&world, &Selection::new(), &runtime);

        assert_eq!(rendered.lights.len(), shining.len(), "render vs shining");
        assert_eq!(shadow.light_count, shining.len(), "shadow vs shining");
        for index in shadow.matrices.keys() {
            assert!(
                *index < shining.len(),
                "shadow index {index} past the {} lights that shine",
                shining.len()
            );
        }
        for (index, light) in shining.iter().enumerate() {
            assert_eq!(
                rendered.lights[index].position, light.position,
                "render light {index} is the light that shines at {index}"
            );
            let alone = ShadowFlow.project(
                &lit_world(&[(light.light.clone(), light.position)]),
                &Selection::new(),
                &runtime,
            );
            assert_eq!(
                published(&shadow, index),
                published(&alone, 0),
                "the matrix at index {index} is the one light {index} publishes alone"
            );
        }
    }
}
