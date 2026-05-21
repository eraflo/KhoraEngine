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

//! Low-resolution shadows lane.
//!
//! Same algorithm as [`super::StandardShadowsLane`] (CSM / spot / cube
//! point), but with smaller atlases — quarter the per-side resolution
//! everywhere. Same output contract: produces a
//! [`khora_data::render::ShadowGpuBindings`] bundle plus
//! [`khora_data::render::ShadowEntries`]; lit consumer lanes never need
//! to know which strategy ran.
//!
//! `ShadowAgent` selects this lane when GORNA reports a tight time /
//! VRAM budget (`StrategyId::LowPower`).

use std::any::Any;

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref};
use khora_core::renderer::GraphicsDevice;
use khora_data::render::RenderWorld;

use super::algo::ShadowsLaneState;

/// Stable strategy name advertised to the agent / GORNA.
pub const STRATEGY_NAME: &str = "LowResShadows";

/// Low-resolution shadows: 512² × 4-layer 2D atlas + 128² × 4-cube cube
/// atlas. Same algorithm and output types as
/// [`super::StandardShadowsLane`]; just lower-resolution textures —
/// VRAM ≈ 1.5 + 0.25 MiB instead of 64 + 24 MiB.
#[derive(Default)]
pub struct LowResShadowsLane {
    state: ShadowsLaneState,
}

impl LowResShadowsLane {
    /// 2D depth atlas resolution (per layer).
    pub const ATLAS_2D_RESOLUTION: u32 = 512;
    /// Number of directional / spot shadow casters per frame.
    pub const ATLAS_2D_MAX_LIGHTS: u32 = 4;
    /// Cube atlas per-face resolution.
    pub const CUBE_FACE_RESOLUTION: u32 = 128;
    /// Number of point shadow casters per frame.
    pub const CUBE_MAX_LIGHTS: u32 = 4;

    /// Creates a new lane in its uninitialised state.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Lane for LowResShadowsLane {
    fn strategy_name(&self) -> &'static str {
        STRATEGY_NAME
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Shadow
    }

    fn estimate_cost(&self, ctx: &LaneContext) -> f32 {
        let render_world = match ctx.get::<Ref<RenderWorld>>() {
            Some(slot) => slot.get(),
            None => return 0.5,
        };
        // Roughly 1/16th the per-texel cost of Standard at the same scene
        // (16× fewer texels per layer), capped so GORNA always sees this
        // strategy as cheaper.
        (super::cost_estimate(render_world) * 0.0625).max(0.05)
    }

    fn on_initialize(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let device = ctx
            .get::<std::sync::Arc<dyn GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let registry = ctx
            .get::<std::sync::Arc<std::sync::Mutex<crate::render_lane::ShaderRegistry>>>()
            .ok_or(LaneError::missing("Arc<Mutex<ShaderRegistry>>"))?
            .clone();
        self.state
            .init_gpu(
                device.as_ref(),
                &registry,
                Self::ATLAS_2D_RESOLUTION,
                Self::ATLAS_2D_MAX_LIGHTS,
                Self::CUBE_FACE_RESOLUTION,
                Self::CUBE_MAX_LIGHTS,
                "LowResShadows",
            )
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        super::execute_shared(
            &self.state,
            Self::ATLAS_2D_MAX_LIGHTS,
            Self::CUBE_MAX_LIGHTS,
            STRATEGY_NAME,
            ctx,
        )
    }

    fn on_shutdown(&self, ctx: &mut LaneContext) {
        if let Some(device) = ctx.get::<std::sync::Arc<dyn GraphicsDevice>>() {
            self.state.shutdown(device.as_ref());
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
