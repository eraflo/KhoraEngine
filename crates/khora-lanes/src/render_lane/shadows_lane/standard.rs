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

//! Full-quality shadows lane.
//!
//! The lane **is** its quality — its atlas dimensions are declared as
//! `const` on the type, not received as configuration. `ShadowAgent`
//! picks this lane (vs [`super::LowResShadowsLane`]) by name when the
//! GORNA budget allows the higher VRAM / time cost.

use std::any::Any;

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref};
use khora_core::renderer::GraphicsDevice;
use khora_data::render::RenderWorld;

use super::algo::ShadowsLaneState;

/// Stable strategy name advertised to the agent / GORNA.
pub const STRATEGY_NAME: &str = "StandardShadows";

/// Full-quality shadows: 2048² × 4-layer 2D atlas + 512² × 4-cube cube
/// atlas. Drives the canonical CSM / spot perspective / 6-pass point
/// pipeline; publishes a [`khora_data::render::ShadowGpuBindings`]
/// bundle plus per-light entries into the per-frame lane context.
#[derive(Default)]
pub struct StandardShadowsLane {
    state: ShadowsLaneState,
}

impl StandardShadowsLane {
    /// 2D depth atlas resolution (per layer).
    pub const ATLAS_2D_RESOLUTION: u32 = 2048;
    /// Number of directional / spot shadow casters per frame.
    pub const ATLAS_2D_MAX_LIGHTS: u32 = 4;
    /// Cube atlas per-face resolution.
    pub const CUBE_FACE_RESOLUTION: u32 = 512;
    /// Number of point shadow casters per frame.
    pub const CUBE_MAX_LIGHTS: u32 = 4;

    /// Creates a new lane in its uninitialised state.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Lane for StandardShadowsLane {
    fn strategy_name(&self) -> &'static str {
        STRATEGY_NAME
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Shadow
    }

    fn estimate_cost(&self, ctx: &LaneContext) -> f32 {
        let render_world = match ctx.get::<Ref<RenderWorld>>() {
            Some(slot) => slot.get(),
            None => return 1.0,
        };
        super::cost_estimate(render_world)
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
                "StandardShadows",
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
