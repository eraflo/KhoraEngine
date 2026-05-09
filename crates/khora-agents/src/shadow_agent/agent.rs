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

//! Defines the ShadowAgent — owns `LaneKind::Shadow` lanes only.
//!
//! Per CLAD, an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA/strategy state.  The graphics device, the GPU mesh cache,
//! the per-frame `RenderWorld`, and the `FrameContext` are all looked up
//! from the [`ServiceRegistry`] each frame — agents are not the owners.

use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::{
    LaneContext, LaneKind, LaneRegistry, Ref, ShadowAtlasView, ShadowComparisonSampler, Slot,
};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;
use khora_data::render::RenderWorld;
use khora_data::GpuCache;
use khora_lanes::render_lane::ShadowPassLane;

const COST_TO_MS_SCALE: f32 = 5.0;

/// The agent responsible for shadow map rendering (`LaneKind::Shadow`).
///
/// Holds **only** its own strategy state — every other dependency
/// (`GraphicsDevice`, `GpuCache`, `RenderWorldStore`, `FrameContext`)
/// is fetched from `EngineContext::services` per frame.
pub struct ShadowAgent {
    /// Shadow lanes — the agent's strategies.
    lanes: LaneRegistry,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Duration of the last shadow pass.
    last_frame_time: Duration,
    /// Total number of shadow frames rendered.
    frame_count: u64,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
    /// Number of `execute` invocations attempted.
    execute_attempts: u64,
}

impl Agent for ShadowAgent {
    fn id(&self) -> AgentId {
        AgentId::ShadowRenderer
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        // Estimate cost from a stub LaneContext. The real RenderWorld lives
        // in the LaneBus at execute time; negotiation runs on the DCC thread
        // without access to the live scene, so we feed the cost estimator a
        // borrowed empty stub.
        let stub_world = RenderWorld::new();
        let mut ctx = LaneContext::new();
        ctx.insert(Ref::new(&stub_world));

        let cost = self
            .lanes
            .find_by_kind(LaneKind::Shadow)
            .first()
            .map(|lane| lane.estimate_cost(&ctx))
            .unwrap_or(1.0);
        let estimated_time = Duration::from_secs_f32((cost * COST_TO_MS_SCALE).max(0.1) / 1000.0);

        // 2048×2048×4 layers @ Depth32Float = 64 MB for the atlas.
        let estimated_vram = 64u64 * 1024 * 1024;

        let mut strategies = Vec::new();
        let fits_constraint = request
            .constraints
            .max_vram_bytes
            .map(|max| estimated_vram <= max)
            .unwrap_or(true);
        if fits_constraint {
            strategies.push(StrategyOption {
                id: StrategyId::HighPerformance,
                estimated_time,
                estimated_vram,
            });
        }

        if strategies.is_empty() {
            strategies.push(StrategyOption {
                id: StrategyId::LowPower,
                estimated_time: Duration::from_millis(1),
                estimated_vram: 0,
            });
        }

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
        // One-shot lane GPU initialization.  Fetch the device, drive
        // lane.on_initialize() once, then drop — the agent does not store it.
        let Some(device_arc) = context.runtime.backends.get::<Arc<dyn GraphicsDevice>>().cloned() else {
            log::warn!("ShadowAgent: graphics device unavailable in on_initialize");
            return;
        };

        let mut init_ctx = LaneContext::new();
        init_ctx.insert(device_arc);
        for lane in self.lanes.all() {
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!(
                    "ShadowAgent: Failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        self.execute_attempts += 1;
        let frame_start = Instant::now();

        // Look up everything from services — the agent owns none of it.
        let Some(device_arc) = context.runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        let Some(gpu_cache) = context.runtime.resources.get::<GpuCache>() else {
            return;
        };
        let gpu_meshes = gpu_cache.inner().clone();

        // Read the per-frame RenderWorld from the LaneBus (RenderFlow).
        let Some(render_world): Option<&RenderWorld> = context.bus.get() else {
            log::warn!("ShadowAgent: no RenderWorld in LaneBus (RenderFlow not run?)");
            return;
        };

        let frame_ctx = context.runtime.resources.get::<Arc<FrameContext>>().cloned();

        // Encode shadow passes into a standalone command buffer.
        let mut encoder = device.create_command_encoder(Some("Shadow Command Encoder"));
        {
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            ctx.insert(gpu_meshes);
            // SAFETY: encoder lives for the entire scope of this block; ctx
            // is dropped before encoder.finish().
            let encoder_slot = Slot::new(encoder.as_mut());
            ctx.insert(unsafe {
                std::mem::transmute::<
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                >(encoder_slot)
            });
            // SAFETY: render_world is borrowed from LaneBus, alive for this
            // entire frame and read-only.
            ctx.insert(Ref::new(render_world));
            // ShadowFlow's pre-computed view-projection matrices, indexed
            // by light position in `RenderWorld.lights`.
            if let Some(shadow_view) = context.bus.get::<khora_data::flow::ShadowView>() {
                ctx.insert(Ref::new(shadow_view));
            }
            // SAFETY: deck is borrowed from EngineContext for the duration
            // of this agent.execute() call; the lane runs synchronously
            // before the slot is dropped.
            ctx.insert(Slot::new(&mut *context.deck));

            for lane in self.lanes.find_by_kind(LaneKind::Shadow) {
                if let Err(e) = lane.execute(&mut ctx) {
                    log::error!(
                        "ShadowAgent: shadow lane {} failed: {}",
                        lane.strategy_name(),
                        e
                    );
                }
            }

            // Hoist atlas view + comparison sampler from the lane's local
            // ctx into the FrameContext so RenderAgent can read them.
            // Cross-agent ordering is now enforced by the scheduler's
            // AgentCompletionMap (RenderAgent declares Hard(ShadowRenderer)).
            if let Some(fctx) = &frame_ctx {
                if let Some(view) = ctx.get::<ShadowAtlasView>().cloned() {
                    fctx.insert(view);
                }
                if let Some(sampler) = ctx.get::<ShadowComparisonSampler>().cloned() {
                    fctx.insert(sampler);
                }
            }
        }
        if let Some(cmd_buf) = encoder.finish() {
            device.submit_command_buffer(cmd_buf);
        } else {
            log::error!(
                "ShadowAgent: encoder.finish() returned None — skipping shadow submission"
            );
        }

        self.last_frame_time = frame_start.elapsed();
        self.frame_count += 1;
    }

    fn report_status(&self) -> AgentStatus {
        let health_score = if self.time_budget.is_zero() || self.frame_count == 0 {
            1.0
        } else {
            let ratio =
                self.time_budget.as_secs_f32() / self.last_frame_time.as_secs_f32().max(0.0001);
            ratio.min(1.0)
        };

        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: self.execute_attempts > 0 && self.frame_count == 0,
            message: format!(
                "shadow_time={:.2}ms",
                self.last_frame_time.as_secs_f32() * 1000.0,
            ),
        }
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OBSERVE],
            default_phase: ExecutionPhase::OBSERVE,
            priority: 1.0,
            importance: AgentImportance::Important,
            dependencies: vec![],
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

impl Default for ShadowAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(ShadowPassLane::default()));

        Self {
            lanes,
            time_budget: Duration::ZERO,
            last_frame_time: Duration::ZERO,
            frame_count: 0,
            current_strategy: StrategyId::HighPerformance,
            execute_attempts: 0,
        }
    }
}
