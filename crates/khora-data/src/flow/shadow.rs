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
//! `RenderWorld.lights`: both are produced by iterating
//! `world.query::<(&Light, &GlobalTransform)>()` and skipping disabled
//! lights, in the same order. The shadow lane and the lit lanes therefore
//! agree on which `i` refers to which light.

use std::collections::HashMap;

use khora_core::math::{Mat4, Vec3, Vec4};
use khora_core::renderer::light::LightType;
use khora_core::Runtime;

use crate::ecs::{GlobalTransform, Light, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::render::{primary_view, ExtractedView};

/// Output of [`ShadowFlow`].
#[derive(Debug, Default, Clone)]
pub struct ShadowView {
    /// Number of enabled lights in the world (regardless of shadow-casting).
    pub light_count: usize,
    /// Per-light view-projection matrices, keyed by the light's position in
    /// `RenderWorld.lights`. Only shadow-casting lights have an entry.
    pub matrices: HashMap<usize, Mat4>,
}

/// Computes shadow view-projection matrices.
#[derive(Default)]
pub struct ShadowFlow;

impl Flow for ShadowFlow {
    type View = ShadowView;

    // No dedicated shadow domain — bucketed under Render for budget purposes.
    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "shadow";

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let camera_view = primary_view(world, runtime);
        let mut matrices = HashMap::new();
        let mut light_count = 0;

        // Mirror RenderFlow's iteration so indices align across views.
        for (light, transform) in world.query::<(&Light, &GlobalTransform)>() {
            if !light.enabled {
                continue;
            }
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

            // Point lights would need a cubemap (6 view-proj matrices per
            // light, sampled with a `texture_cube` lookup in lit shaders).
            // That pipeline isn't built yet, so we deliberately skip point
            // lights here rather than emit an identity matrix that would
            // produce undefined shadow sampling. Tracked in the audit as
            // P2.a — cubemap shadows.
            if matches!(light.light_type, LightType::Point(_)) {
                continue;
            }

            let Some(camera) = camera_view.as_ref() else {
                continue;
            };

            let position = transform.0.translation();
            let direction = match &light.light_type {
                LightType::Directional(d) => transform.0.rotation() * d.direction,
                LightType::Spot(s) => transform.0.rotation() * s.direction,
                LightType::Point(_) => unreachable!("filtered above"),
            };

            let view_proj =
                compute_shadow_view_proj(&light.light_type, position, direction, camera);
            matrices.insert(light_index, view_proj);
        }

        ShadowView {
            light_count,
            matrices,
        }
    }
}

register_flow!(ShadowFlow);

// ─── pure shadow-matrix math (moved out of `ShadowPassLane`) ─────────

fn compute_shadow_view_proj(
    light_type: &LightType,
    position: Vec3,
    direction: Vec3,
    camera: &ExtractedView,
) -> Mat4 {
    match light_type {
        LightType::Directional(_) => directional_shadow_view_proj(direction, camera),
        LightType::Spot(sl) => {
            let up = if direction.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let view =
                Mat4::look_at_rh(position, position + direction, up).unwrap_or(Mat4::IDENTITY);
            let proj = Mat4::perspective_rh_zo(sl.outer_cone_angle * 2.0, 1.0, 0.1, sl.range);
            proj * view
        }
        // Point lights are filtered out before reaching this function
        // (cubemap shadows not yet wired); see the call site in `project`.
        LightType::Point(_) => unreachable!(
            "point lights are filtered out in ShadowFlow::project — \
             cubemap shadows are not yet wired"
        ),
    }
}

/// CSM (cascaded shadow map) view-projection for a directional light.
/// Lifted verbatim from the previous `ShadowPassLane::calculate_shadow_view_proj`
/// so behaviour is identical — the only change is *where* it lives.
fn directional_shadow_view_proj(direction: Vec3, camera: &ExtractedView) -> Mat4 {
    // 1. Camera frustum corners in world space.
    let inv_view_proj = camera.view_proj.inverse().unwrap_or(Mat4::IDENTITY);
    let mut corners = Vec::with_capacity(8);
    for x in &[-1.0, 1.0] {
        for y in &[-1.0, 1.0] {
            for z in &[0.0, 1.0] {
                let pt = inv_view_proj * Vec4::new(*x, *y, *z, 1.0);
                corners.push(pt.truncate() / pt.w);
            }
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
