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

//! Defines the OverlayAgent — owns overlay / debug `LaneKind::Render` lanes.
//!
//! Per CLAD an Agent owns exactly one domain. OverlayAgent's domain is
//! "post-main-render overlay passes". Lanes here render into the same
//! color target as `RenderAgent`, but with `depth-read-only` semantics
//! and alpha blending — they overlay, they don't replace.
//!
//! Unlike `RenderAgent`, OverlayAgent does **not** pick one strategy
//! per frame. Each registered lane is independent and runs every frame
//! it has work to do — `GizmoLane` skips if no gizmos were published,
//! `WireframeLane` runs when the debug flag is on, etc.

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::{
    Agent, AgentAccess, AgentDependency, AgentImportance, DependencyKind, ExecutionPhase,
    ExecutionTiming,
};
use khora_core::control::gorna::{
    measured_frame_time_ms, AgentFrameStatusMap, AgentId, AgentStatus, NegotiationRequest,
    NegotiationResponse, ResourceBudget, StrategyId, StrategyOption,
};
use khora_core::lane::{ClearColor, ColorTarget, DepthTarget, LaneContext, LaneRegistry, Slot};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;
use khora_data::assets::Assets;
use khora_data::render::{
    OverlayPassSlot, PassContribution, PassDescriptor, RenderWorld, ResourceId,
};
use khora_data::AssetStore;
use std::sync::RwLock;

/// The agent responsible for overlay / debug rendering passes.
///
/// Owns a [`LaneRegistry`] of overlay lanes (gizmo, wireframe, emissive,
/// …). Each lane is asked to run every frame; lanes that have nothing
/// to do early-out internally rather than being gated at the agent
/// level. This keeps the agent generic — it never knows what each lane
/// needs to consult to decide.
pub struct OverlayAgent {
    /// Registered overlay lanes.
    lanes: LaneRegistry,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
    /// Shared, scheduler-written per-agent frame metrics, read in
    /// `report_status`; the agent holds no per-frame counters.
    frame_status: Option<AgentFrameStatusMap>,
}

impl Agent for OverlayAgent {
    fn id(&self) -> AgentId {
        AgentId::Overlay
    }

    /// Reads only the `LaneBus` and shared read-only resources, records into its
    /// own encoder, and buffers its pass into its `OutputDeck` — no `World`, no
    /// shared mutable resource. Eligible to run concurrently with any wave.
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }

    /// Buffers its pass into the [`OverlayPassSlot`] deck slot.
    fn deck_writes(&self) -> Vec<std::any::TypeId> {
        vec![std::any::TypeId::of::<OverlayPassSlot>()]
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // Overlay passes are bounded by host application activity (editor
        // gizmos, debug flags). They don't compete for the main render
        // budget — quote a small flat overhead and accept any of the
        // standard strategy IDs.
        let strategies = vec![StrategyOption {
            id: StrategyId::Balanced,
            estimated_time: Duration::from_micros(500),
            estimated_vram: 1024 * 1024,
        }];
        NegotiationResponse {
            strategies,
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.time_budget = budget.time_limit;
        self.current_strategy = budget.strategy_id;
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        self.frame_status = context
            .runtime
            .resources
            .get::<AgentFrameStatusMap>()
            .cloned();

        let Some(device_arc) = context
            .runtime
            .backends
            .get::<Arc<dyn GraphicsDevice>>()
            .cloned()
        else {
            log::warn!("OverlayAgent: graphics device unavailable in on_initialize");
            return;
        };
        let pipeline_system = context
            .runtime
            .resources
            .get::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .cloned();

        let mut init_ctx = LaneContext::new();
        init_ctx.insert(device_arc);
        if let Some(ps) = pipeline_system {
            init_ctx.insert(ps);
        }
        for lane in self.lanes.all() {
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!(
                    "OverlayAgent: failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        if self.lanes.is_empty() {
            // Nothing to do — no overlay lanes registered. Common when
            // the host application doesn't need debug viz.
            return;
        }

        let Some(device_arc) = context.runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        let Some(asset_store) = context.runtime.resources.get::<AssetStore>() else {
            return;
        };
        let gpu_meshes: Arc<RwLock<Assets<GpuMesh>>> = asset_store.store::<GpuMesh>();

        let render_world: Option<&RenderWorld> = context.bus.get();

        // Read frame targets — overlay lanes draw into the same color
        // target as the main render and use the existing depth buffer
        // read-only.
        let Some(fctx) = context.runtime.resources.get::<Arc<FrameContext>>() else {
            log::warn!("OverlayAgent: no FrameContext in services");
            return;
        };
        let Some(color_target) = fctx.get::<ColorTarget>().map(|a| *a) else {
            return;
        };
        let depth_target = fctx.get::<DepthTarget>().map(|a| *a);
        let clear_color = fctx
            .get::<ClearColor>()
            .map(|a| *a)
            .unwrap_or_else(|| ClearColor(khora_core::math::LinearRgba::new(0.0, 0.0, 0.0, 0.0)));

        // Encode every overlay lane's commands into a single command
        // buffer, attached to the FrameGraph as a single OverlayPass.
        let mut encoder = device.create_command_encoder(Some("Khora Overlay Encoder"));
        {
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            ctx.insert(gpu_meshes.clone());
            // SAFETY: encoder is alive for the whole block; ctx is dropped
            // before encoder.finish() consumes it.
            let encoder_slot = Slot::new(encoder.as_mut());
            ctx.insert(unsafe {
                std::mem::transmute::<
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                >(encoder_slot)
            });
            // RenderWorld carries the primary view GizmoLane needs for
            // its camera matrix.
            if let Some(rw) = render_world {
                ctx.insert(khora_core::lane::Ref::new(rw));
            }
            ctx.insert(Slot::new(&mut *context.deck));
            ctx.insert(color_target);
            if let Some(dt) = depth_target {
                ctx.insert(dt);
            }
            ctx.insert(clear_color);

            // The host application (editor, debug tooling) publishes
            // gizmo lines into this shared frame. The engine only owns
            // the mechanism — it never produces the data itself, so the
            // editor stays a pure consumer of engine APIs (no
            // engine-internal `EditorAgent`).
            if let Some(gizmos) = context
                .runtime
                .resources
                .get::<khora_lanes::render_lane::SharedGizmoFrame>()
                .cloned()
            {
                ctx.insert(gizmos);
            }
            // Editor-grid opt-in — same mechanism: the host app enables
            // it, `GridLane` consumes it; absent ⇒ no grid.
            if let Some(grid_cfg) = context
                .runtime
                .resources
                .get::<khora_lanes::render_lane::SharedGridConfig>()
                .cloned()
            {
                ctx.insert(grid_cfg);
            }
            // Wireframe debug overlay — same opt-in mechanism.
            if let Some(wireframe_cfg) = context
                .runtime
                .resources
                .get::<khora_lanes::render_lane::SharedWireframeConfig>()
                .cloned()
            {
                ctx.insert(wireframe_cfg);
            }

            for lane in self.lanes.all() {
                if let Err(e) = lane.execute(&mut ctx) {
                    log::error!("OverlayAgent: lane {} failed: {}", lane.strategy_name(), e);
                }
            }
        }

        if let Some(cmd_buf) = encoder.finish() {
            let descriptor = PassDescriptor::new("OverlayPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth);
            // Buffer into the deck; the engine folds it into the FrameGraph after
            // the wave. With no shared-graph lock and no other shared mutable
            // write, OverlayAgent is `AgentAccess::Isolated`.
            context.deck.slot::<OverlayPassSlot>().0 = Some(PassContribution {
                descriptor,
                command_buffer: cmd_buf,
            });
        }
    }

    fn report_status(&self) -> AgentStatus {
        let measured_time_ms = measured_frame_time_ms(&self.frame_status, self.id());
        let health_score = if self.time_budget.is_zero() || measured_time_ms <= 0.0 {
            1.0
        } else {
            (self.time_budget.as_secs_f32() * 1000.0 / measured_time_ms).min(1.0)
        };
        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!(
                "overlay_lanes={} frame_time={measured_time_ms:.2}ms",
                self.lanes.len(),
            ),
        }
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            // Lower priority than `Renderer` (1.0) so the scheduler
            // dispatches the main render before the overlay.
            priority: 0.5,
            importance: AgentImportance::Optional,
            // The OverlayPass draws on top of the main render's color
            // target, so we need the Renderer to have submitted first.
            dependencies: vec![AgentDependency {
                target: AgentId::Renderer,
                kind: DependencyKind::Hard,
                condition: None,
            }],
            fixed_timestep: None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Default for OverlayAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        // Order matters — overlays composite in registration order:
        // grid is the backdrop, wireframe is debug viz, gizmo is the editor
        // handles on top.
        lanes.register(Box::new(khora_lanes::render_lane::GridLane::default()));
        lanes.register(Box::new(khora_lanes::render_lane::WireframeLane::default()));
        lanes.register(Box::new(khora_lanes::render_lane::GizmoLane::default()));

        Self {
            lanes,
            time_budget: Duration::ZERO,
            current_strategy: StrategyId::Balanced,
            frame_status: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_agent_registers_overlay_lanes() {
        let agent = OverlayAgent::default();
        assert_eq!(agent.lanes.len(), 3);
        assert!(agent.lanes.get("Grid").is_some());
        assert!(agent.lanes.get("Wireframe").is_some());
        assert!(agent.lanes.get("Gizmo").is_some());
    }

    #[test]
    fn overlay_agent_reports_overlay_id() {
        let agent = OverlayAgent::default();
        assert_eq!(agent.id(), AgentId::Overlay);
    }

    #[test]
    fn overlay_agent_runs_in_output_phase_after_renderer() {
        let agent = OverlayAgent::default();
        let timing = agent.execution_timing();
        assert_eq!(timing.default_phase, ExecutionPhase::OUTPUT);
        assert_eq!(timing.dependencies.len(), 1);
        assert_eq!(timing.dependencies[0].target, AgentId::Renderer);
    }
}
