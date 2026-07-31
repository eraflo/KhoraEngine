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

//! Shadow lanes for [`ShadowAgent`].
//!
//! Per the SAA lane model, each lane in this module is a **distinct
//! quality strategy**, all implementing the same shadow domain:
//!
//! - [`StandardShadowsLane`] — full quality (2048² atlas + 512² cube)
//! - [`MediumShadowsLane`]   — same algorithm, half resolution (1024² + 256²)
//! - [`LowResShadowsLane`]   — same algorithm, quarter resolution (512² + 128²)
//!
//! All produce identical output types (`ShadowGpuBindings` +
//! `ShadowEntries`); lit consumer lanes never need to know which one
//! ran. The agent picks one per frame via `apply_budget`.
//!
//! Shared infrastructure (atlas creation, pass recording, draw-cmd
//! building) lives in [`algo`] as free functions that take dimensions
//! as parameters — never as a config struct injected by the agent.

pub mod algo;
mod low_res;
mod medium;
mod standard;

pub use low_res::{LowResShadowsLane, STRATEGY_NAME as LOW_RES_STRATEGY_NAME};
pub use medium::{MediumShadowsLane, STRATEGY_NAME as MEDIUM_STRATEGY_NAME};
pub use standard::{StandardShadowsLane, STRATEGY_NAME as STANDARD_STRATEGY_NAME};

use khora_core::lane::{LaneContext, LaneError, Ref, Slot};
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::{traits::CommandEncoder, GraphicsDevice};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;

use algo::ShadowsLaneState;

/// Cost estimate shared by both strategies — roughly `casters × meshes`
/// with point lights weighted 6× (six cube passes). Each lane scales
/// this further to reflect its own quality.
pub(crate) fn cost_estimate(render_world: &RenderWorld) -> f32 {
    let mut cube_passes = 0u32;
    for light in &render_world.lights {
        let (enabled, is_point) = match &light.light_type {
            khora_core::renderer::light::LightType::Directional(l) => (l.shadow_enabled, false),
            khora_core::renderer::light::LightType::Spot(l) => (l.shadow_enabled, false),
            khora_core::renderer::light::LightType::Point(l) => (l.shadow_enabled, true),
        };
        if enabled {
            cube_passes += if is_point { 6 } else { 1 };
        }
    }
    (cube_passes as f32) * (render_world.meshes.len() as f32) * 0.001
}

/// Shared `Lane::execute` body — each concrete strategy delegates here
/// with its own `(atlas_2d_max_lights, cube_max_lights)` constants and
/// `strategy_label` for diagnostics. Eliminates duplication while
/// keeping each lane's quality intrinsic to its own type.
pub(crate) fn execute_shared(
    state: &ShadowsLaneState,
    atlas_2d_max_lights: u32,
    cube_max_lights: u32,
    strategy_label: &'static str,
    ctx: &mut LaneContext,
) -> Result<(), LaneError> {
    // Phase 1: Render shadow maps.
    {
        let device = ctx
            .get::<std::sync::Arc<dyn GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let gpu_meshes = ctx
            .get::<std::sync::Arc<std::sync::RwLock<Assets<GpuMesh>>>>()
            .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?
            .clone();
        let encoder = ctx
            .get::<Slot<dyn CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
            .get();
        let render_world = ctx
            .get::<Ref<RenderWorld>>()
            .ok_or(LaneError::missing("Ref<RenderWorld>"))?
            .get();
        let shadow_view = ctx
            .get::<Ref<khora_data::flow::ShadowView>>()
            .map(|r| r.get());

        state.render(
            atlas_2d_max_lights,
            cube_max_lights,
            strategy_label,
            render_world,
            shadow_view,
            device.as_ref(),
            encoder,
            &gpu_meshes,
        );
    }

    // Phase 2: Publish a single `ShadowFrame` slot into the per-frame
    // `OutputDeck`. This is the **only** cross-lane channel — no
    // `LaneContext::insert`, no `FrameContext` hoist, no shared
    // resource. Lit consumer lanes read `deck.slot::<ShadowFrame>()`.
    if let Some(deck_slot) = ctx.get::<Slot<khora_core::lane::OutputDeck>>() {
        let deck = deck_slot.get();
        let frame = deck.slot::<khora_core::renderer::api::shadow::ShadowFrame>();
        frame.bindings = state.shadow_bindings();
        if let Ok(results) = state.shadow_results.read() {
            frame.entries.0.clear();
            for (i, entry) in results.iter() {
                frame.entries.insert(*i, entry.clone());
            }
        }
    }

    Ok(())
}
