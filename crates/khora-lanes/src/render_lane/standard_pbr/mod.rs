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

//! Standard PBR rendering lane — Cook-Torrance BRDF, multi-light, with
//! shadow sampling. Alternative strategy to `LitForwardLane` /
//! `ForwardPlusLane`, registered under
//! [`RenderAgent`](khora_agents::render_agent).
//!
//! The pipeline is composed from `khora::pipelines::standard_pbr`,
//! which shares the same group(3) uniform / shadow layout as
//! `LitForward` — the only difference is the BRDF in the fragment
//! shader. Consequently this lane reuses the lit lane's render path
//! one-for-one, plumbed through a shared free function so the two
//! lanes never drift apart structurally.

use crate::render_lane::util::DynamicUniformRingBuffer;
use crate::render_lane::util::UniformRingBuffer;
use gpu::init_gpu_resources;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::traits::CommandEncoder;
use render::render_pbr;
use std::sync::{Mutex, OnceLock};

mod gpu;
mod render;

/// Full PBR (Cook-Torrance) lit lane.
///
/// Per CLAD this struct holds only the lane's persistent state — the
/// init / render bodies are private free functions in this module, the
/// trait impl below dispatches to them. No inherent methods other than
/// what the `Lane` and `Default` traits require.
#[derive(Default)]
pub struct StandardPbrLane {
    pipeline: OnceLock<RenderPipelineId>,
    /// Backend pipeline system, retained so the render path can resolve the
    /// per-material-variant pipeline (cheap cache hit). Set in init.
    pipeline_system: OnceLock<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>,
    camera_layout: OnceLock<BindGroupLayoutId>,
    model_layout: OnceLock<BindGroupLayoutId>,
    material_layout: OnceLock<BindGroupLayoutId>,
    /// Full lighting + shadows bind group layout (group 3).
    light_layout: OnceLock<BindGroupLayoutId>,
    /// Single-binding layout used by the lighting ring buffer.
    lighting_buffer_layout: OnceLock<BindGroupLayoutId>,
    camera_ring: Mutex<Option<UniformRingBuffer>>,
    lighting_ring: Mutex<Option<UniformRingBuffer>>,
    /// Per-frame model (transform) uniforms — written once, bound at a
    /// dynamic offset per draw (no per-draw buffer/bind-group churn).
    model_ring: Mutex<Option<DynamicUniformRingBuffer>>,
}

impl khora_core::lane::Lane for StandardPbrLane {
    fn strategy_name(&self) -> &'static str {
        "StandardPbr"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Render
    }

    fn on_initialize(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(khora_core::lane::LaneError::missing(
                "Arc<dyn GraphicsDevice>",
            ))?
            .clone();
        let pipeline_system = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .ok_or(khora_core::lane::LaneError::missing(
                "Arc<dyn PipelineSystem>",
            ))?
            .clone();
        // Retain the system so the render hot path can resolve a pipeline per
        // material variant (a cheap cache hit after first compile).
        let _ = self.pipeline_system.set(pipeline_system.clone());
        init_gpu_resources(self, device.as_ref(), pipeline_system.as_ref())
            .map_err(|e| khora_core::lane::LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::LaneError;
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let gpu_meshes = ctx
            .get::<std::sync::Arc<
                std::sync::RwLock<
                    khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
                >,
            >>()
            .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?
            .clone();
        let mut encoder_guard = ctx
            .slot::<dyn khora_core::renderer::traits::CommandEncoder>()
            .ok_or(LaneError::missing("&mut dyn CommandEncoder"))?;
        let encoder = &mut *encoder_guard;
        let render_world = ctx
            .get_ref::<khora_data::render::RenderWorld>()
            .ok_or(LaneError::missing("&RenderWorld"))?;
        let color_target = ctx
            .get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?
            .0;
        let depth_target = ctx
            .get::<khora_core::lane::DepthTarget>()
            .ok_or(LaneError::missing("DepthTarget"))?
            .0;
        let clear_color = ctx
            .get::<khora_core::lane::ClearColor>()
            .ok_or(LaneError::missing("ClearColor"))?
            .0;
        let render_ctx = khora_core::renderer::api::core::RenderContext::new(
            &color_target,
            Some(&depth_target),
            clear_color,
        );

        let (shadow_entries, shadow_bindings) = ctx
            .slot::<khora_core::lane::OutputDeck>()
            .map(|mut s| {
                let frame = s.slot::<khora_core::renderer::api::shadow::ShadowFrame>();
                (frame.entries.clone(), frame.bindings)
            })
            .unwrap_or_default();

        // Image-based lighting bindings — baked once at startup, forwarded into
        // the lane ctx by the render agent (static after the bake).
        let ibl_bindings = ctx
            .get::<khora_core::renderer::api::ibl::IblGpuBindings>()
            .copied();

        render_pbr(
            self,
            render_world,
            &shadow_entries,
            shadow_bindings,
            ibl_bindings,
            device.as_ref(),
            encoder,
            ctx.slot_as::<khora_data::render::TransparentEncoder, dyn khora_core::renderer::traits::CommandEncoder>()
                .as_deref_mut()
                .map(|encoder| encoder as &mut dyn CommandEncoder),
            &render_ctx,
            &gpu_meshes,
        );
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests;
