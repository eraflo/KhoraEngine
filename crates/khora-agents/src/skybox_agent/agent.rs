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

//! Defines the SkyboxAgent — owns the environment-background `LaneKind::Render`
//! pass.

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
use khora_core::lane::{ColorTarget, DepthTarget, LaneContext, LaneRegistry, Slot};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;
use khora_data::render::{PassContribution, PassDescriptor, RenderWorld, ResourceId, SkyboxPassSlot};

/// The agent responsible for the skybox / environment-background pass.
///
/// Owns a single-lane [`LaneRegistry`] (the [`SkyboxLane`]). Structurally it
/// mirrors `OverlayAgent` — `Isolated`, buffers a pass into a deck slot — but
/// it runs its lane unconditionally (no host opt-in) and carries a higher
/// importance so the background sky survives budget pressure.
///
/// [`SkyboxLane`]: khora_lanes::render_lane::SkyboxLane
pub struct SkyboxAgent {
    /// The single skybox lane, held in a registry for parity with the other
    /// render agents.
    lanes: LaneRegistry,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
    /// Shared, scheduler-written per-agent frame metrics, read in
    /// `report_status`; the agent holds no per-frame counters.
    frame_status: Option<AgentFrameStatusMap>,
}

impl Agent for SkyboxAgent {
    fn id(&self) -> AgentId {
        AgentId::Skybox
    }

    /// Reads only the `LaneBus` + shared read-only resources (the IBL bake's
    /// bindings), records into its own encoder, and buffers its pass into its
    /// `OutputDeck` — no `World`, no shared mutable resource.
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }

    /// Buffers its pass into the [`SkyboxPassSlot`] deck slot.
    fn deck_writes(&self) -> Vec<std::any::TypeId> {
        vec![std::any::TypeId::of::<SkyboxPassSlot>()]
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // A single fixed background pass — quote a small flat overhead and
        // accept the balanced strategy (there is nothing to trade off).
        let strategies = vec![StrategyOption {
            id: StrategyId::Balanced,
            estimated_time: Duration::from_micros(300),
            estimated_vram: 0,
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
            log::warn!("SkyboxAgent: graphics device unavailable in on_initialize");
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
                    "SkyboxAgent: failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        let Some(device_arc) = context.runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        // The env cube published by the IBL bake. Absent until the bake has run
        // (first frames) ⇒ no background yet; the scene's clear color shows.
        let ibl = context
            .runtime
            .resources
            .get::<khora_data::IblBaker>()
            .and_then(|baker| baker.bindings());
        let Some(ibl) = ibl else {
            return;
        };

        let render_world: Option<&RenderWorld> = context.bus.get();

        // Frame targets — the skybox draws into the same color target as the
        // main render and depth-tests against the existing depth buffer.
        let Some(fctx) = context.runtime.resources.get::<Arc<FrameContext>>() else {
            log::warn!("SkyboxAgent: no FrameContext in services");
            return;
        };
        let Some(color_target) = fctx.get::<ColorTarget>().map(|a| *a) else {
            return;
        };
        let Some(depth_target) = fctx.get::<DepthTarget>().map(|a| *a) else {
            return;
        };

        let mut encoder = device.create_command_encoder(Some("Khora Skybox Encoder"));
        {
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            // SAFETY: encoder is alive for the whole block; ctx is dropped
            // before encoder.finish() consumes it.
            let encoder_slot = Slot::new(encoder.as_mut());
            ctx.insert(unsafe {
                std::mem::transmute::<
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                >(encoder_slot)
            });
            if let Some(rw) = render_world {
                ctx.insert(khora_core::lane::Ref::new(rw));
            }
            ctx.insert(color_target);
            ctx.insert(depth_target);
            // The env-cube bindings the SkyboxLane samples. LaneContext does not
            // reach `runtime.resources`, so the agent forwards them explicitly.
            ctx.insert(ibl);

            for lane in self.lanes.all() {
                if let Err(e) = lane.execute(&mut ctx) {
                    log::error!("SkyboxAgent: lane {} failed: {}", lane.strategy_name(), e);
                }
            }
        }

        if let Some(cmd_buf) = encoder.finish() {
            let descriptor = PassDescriptor::new("SkyboxPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth);
            context.deck.slot::<SkyboxPassSlot>().0 = Some(PassContribution {
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
            message: format!("skybox frame_time={measured_time_ms:.2}ms"),
        }
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            // Lower than `Renderer` (1.0) so the scene draws first; higher than
            // the optional overlays (0.5) since the sky is a core background.
            priority: 0.6,
            // Not debug viz — keep the background under budget pressure.
            importance: AgentImportance::Important,
            // The SkyboxPass loads the scene's color + depth, so the Renderer
            // must have submitted first.
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

impl Default for SkyboxAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(
            khora_lanes::render_lane::SkyboxLane::default(),
        ));
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
    fn skybox_agent_registers_its_lane() {
        let agent = SkyboxAgent::default();
        assert_eq!(agent.lanes.len(), 1);
        assert!(agent.lanes.get("Skybox").is_some());
    }

    #[test]
    fn skybox_agent_reports_skybox_id() {
        let agent = SkyboxAgent::default();
        assert_eq!(agent.id(), AgentId::Skybox);
    }

    #[test]
    fn skybox_agent_runs_in_output_phase_after_renderer() {
        let agent = SkyboxAgent::default();
        let timing = agent.execution_timing();
        assert_eq!(timing.default_phase, ExecutionPhase::OUTPUT);
        assert_eq!(timing.importance, AgentImportance::Important);
        assert_eq!(timing.dependencies.len(), 1);
        assert_eq!(timing.dependencies[0].target, AgentId::Renderer);
    }
}
