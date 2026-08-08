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
//! Per CLAD an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA / strategy state. The agent registers all available shadow
//! strategies as separate lanes; per-frame it selects **one** lane based
//! on the budget GORNA assigned via `apply_budget`.
//!
//! Strategies:
//!
//! - [`StandardShadowsLane`] — full quality, 2048² 2D atlas + 512² cube atlas.
//! - [`MediumShadowsLane`] — same algorithm, half resolution (1024² + 256²),
//!   the `Balanced` middle rung.
//! - [`LowResShadowsLane`] — same algorithm, quarter resolution (512² + 128²)
//!   for tight time / VRAM budgets.
//!
//! All produce the same `ShadowGpuBindings` + `ShadowEntries` contract;
//! lit consumer lanes are agnostic about which one ran.

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::{Agent, AgentImportance, Contention, ExecutionPhase, ExecutionTiming};
use khora_core::control::gorna::{
    measured_frame_time_ms, AgentFrameStatusMap, AgentId, AgentStatus, NegotiationRequest,
    NegotiationResponse, ResourceBudget, StrategyId, StrategyOption,
};
use khora_core::lane::{LaneContext, LaneRegistry, Ref, Slot};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;
use khora_data::render::RenderWorld;
use khora_data::AssetStore;
use khora_lanes::render_lane::shadows_lane::{
    LOW_RES_STRATEGY_NAME, MEDIUM_STRATEGY_NAME, STANDARD_STRATEGY_NAME,
};
use khora_lanes::render_lane::{LowResShadowsLane, MediumShadowsLane, StandardShadowsLane};

const COST_TO_MS_SCALE: f32 = 5.0;

/// Strategy slot mirroring the `Lane` family registered on the agent.
///
/// One value = one fully-featured shadow pipeline. The agent stores the
/// currently-selected variant so `execute()` knows which lane to invoke
/// (the registry holds them both, but only one runs per frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowStrategy {
    /// Full-quality pipeline: 2048² × 4-layer 2D atlas + 512² × 4-cube
    /// cube atlas. Maps to [`StandardShadowsLane`].
    Standard,
    /// Half resolution (1024² × 4-layer + 256² × 4-cube) — the `Balanced`
    /// middle rung. Maps to [`MediumShadowsLane`].
    Medium,
    /// Quarter resolution (512² × 4-layer + 128² × 4-cube).
    /// Maps to [`LowResShadowsLane`].
    LowRes,
}

impl ShadowStrategy {
    /// Returns the stable strategy name advertised by the matching lane.
    pub fn lane_name(self) -> &'static str {
        match self {
            ShadowStrategy::Standard => STANDARD_STRATEGY_NAME,
            ShadowStrategy::Medium => MEDIUM_STRATEGY_NAME,
            ShadowStrategy::LowRes => LOW_RES_STRATEGY_NAME,
        }
    }

    /// Maps a GORNA-issued [`StrategyId`] onto a concrete shadow strategy.
    /// Each tier gets a genuinely different pipeline so budget changes are
    /// observable in quality and cost.
    fn from_strategy_id(id: StrategyId) -> Self {
        match id {
            StrategyId::HighPerformance => ShadowStrategy::Standard,
            StrategyId::Balanced => ShadowStrategy::Medium,
            StrategyId::LowPower => ShadowStrategy::LowRes,
            StrategyId::Custom(_) => ShadowStrategy::Standard,
        }
    }
}

/// The agent responsible for shadow map rendering (`LaneKind::Shadow`).
///
/// Holds **only** its own strategy state — every other dependency
/// (`GraphicsDevice`, `GpuCache`, `RenderWorld`, `FrameContext`) is
/// fetched from `EngineContext::services` per frame.
pub struct ShadowAgent {
    /// Registered strategies — one lane per [`ShadowStrategy`] value.
    lanes: LaneRegistry,
    /// Strategy currently selected by GORNA.
    strategy: ShadowStrategy,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
    /// Shared, scheduler-written per-agent frame metrics, read in
    /// `report_status`; the agent holds no per-frame counters.
    frame_status: Option<AgentFrameStatusMap>,
}

impl Agent for ShadowAgent {
    fn id(&self) -> AgentId {
        AgentId::ShadowRenderer
    }

    fn contention(&self) -> Contention {
        Contention::none()
            .reading::<AssetStore>()
            .reading::<Arc<FrameContext>>()
            .reading::<AgentFrameStatusMap>()
            .reading::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .reading::<Arc<dyn GraphicsDevice>>()
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        // Estimate cost from a stub LaneContext. The real RenderWorld lives
        // in the LaneBus at execute time; negotiation runs on the DCC thread
        // without access to the live scene, so we feed the cost estimator a
        // borrowed empty stub.
        let stub_world = RenderWorld::new();
        let mut ctx = LaneContext::new();
        ctx.insert(Ref::new(&stub_world));

        let lane_time = |name: &str, default_cost: f32| {
            let cost = self
                .lanes
                .get(name)
                .map(|lane| lane.estimate_cost(&ctx))
                .unwrap_or(default_cost);
            Duration::from_secs_f32((cost * COST_TO_MS_SCALE).max(0.1) / 1000.0)
        };

        // Per-tier VRAM at Depth32Float: 2D atlas (res² × 4 layers × 4B) +
        // cube atlas (face² × 24 layers × 4B).
        //   Standard: 64 + 24 MiB · Medium: 16 + 6 MiB · LowRes: 4 + 1.5 MiB
        let std_vram = (64 + 24) * 1024 * 1024_u64;
        let med_vram = (16 + 6) * 1024 * 1024_u64;
        let low_vram = 4 * 1024 * 1024 + 3 * 512 * 1024_u64;

        let fits = |vram: u64| {
            request
                .constraints
                .max_vram_bytes
                .map(|max| vram <= max)
                .unwrap_or(true)
        };

        let mut strategies = Vec::new();
        if fits(std_vram) {
            strategies.push(StrategyOption {
                id: StrategyId::HighPerformance,
                estimated_time: lane_time(STANDARD_STRATEGY_NAME, 1.0),
                estimated_vram: std_vram,
            });
        }
        if fits(med_vram) {
            strategies.push(StrategyOption {
                id: StrategyId::Balanced,
                estimated_time: lane_time(MEDIUM_STRATEGY_NAME, 0.75),
                estimated_vram: med_vram,
            });
        }

        // LowRes is always offered so the agent never returns an empty
        // strategy set — it is the floor GORNA can fall back to.
        strategies.push(StrategyOption {
            id: StrategyId::LowPower,
            estimated_time: lane_time(LOW_RES_STRATEGY_NAME, 0.5),
            estimated_vram: low_vram,
        });

        NegotiationResponse {
            strategies,
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.time_budget = budget.time_limit;
        self.current_strategy = budget.strategy_id;
        self.strategy = ShadowStrategy::from_strategy_id(budget.strategy_id);
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        self.frame_status = context.resource::<AgentFrameStatusMap>().cloned();

        // One-shot lane GPU initialization for every registered strategy.
        // Strategies are cheap to keep idle — only the selected lane
        // executes per frame, but each one needs its own resources ready
        // when the agent eventually picks it.
        let Some(device_arc) = context.resource::<Arc<dyn GraphicsDevice>>().cloned() else {
            log::warn!("ShadowAgent: graphics device unavailable in on_initialize");
            return;
        };
        let pipeline_system = context
            .resource::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .cloned();

        let mut init_ctx = LaneContext::new();
        init_ctx.insert(device_arc);
        if let Some(ps) = pipeline_system {
            init_ctx.insert(ps);
        }
        for lane in self.lanes.all() {
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!(
                    "ShadowAgent: failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        // Look up everything from services — the agent owns none of it.
        let Some(device_arc) = context.resource::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        let Some(asset_store) = context.resource::<AssetStore>() else {
            return;
        };
        let gpu_meshes = asset_store.store::<GpuMesh>();

        let Some(render_world): Option<&RenderWorld> = context.bus.get() else {
            log::warn!("ShadowAgent: no RenderWorld in LaneBus (RenderFlow not run?)");
            return;
        };

        let frame_ctx = context.resource::<Arc<FrameContext>>().cloned();

        // Encode shadow passes into a standalone command buffer.
        let mut encoder = device.create_command_encoder(Some("Shadow Command Encoder"));
        {
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            ctx.insert(gpu_meshes);
            // The encoder outlives `ctx`, which is dropped before it is
            // finished — the contract `Slot::for_encoder` states.
            ctx.insert(Slot::for_encoder(encoder.as_mut()));
            ctx.insert(Ref::new(render_world));
            if let Some(shadow_view) = context.bus.get::<khora_data::flow::ShadowView>() {
                ctx.insert(Ref::new(shadow_view));
            }
            // SAFETY: deck is borrowed from EngineContext for the duration
            // of this agent.execute() call; the lane runs synchronously
            // before the slot is dropped.
            ctx.insert(Slot::new(&mut *context.deck));

            // Pick exactly one lane (the strategy GORNA selected) and run
            // it. The unselected lanes stay idle for this frame — their
            // resources remain allocated but nothing renders into them.
            let lane_name = self.strategy.lane_name();
            if let Some(lane) = self.lanes.get(lane_name) {
                if let Err(e) = lane.execute(&mut ctx) {
                    log::error!(
                        "ShadowAgent: shadow lane {} failed: {}",
                        lane.strategy_name(),
                        e
                    );
                }
            } else {
                log::error!(
                    "ShadowAgent: selected strategy {:?} has no registered lane",
                    self.strategy
                );
            }

            // No `FrameContext` hoist — the lane has already published
            // a `ShadowFrame` slot into the per-frame `OutputDeck`.
            // Consumer lit lanes read `deck.slot::<ShadowFrame>()`
            // directly. This is the only cross-agent channel for
            // shadow data and complies with CLAD (input via Bus, output
            // via Deck, no side-channels).
        }
        let _ = frame_ctx; // kept for any future per-frame resource use
        if let Some(cmd_buf) = encoder.finish() {
            device.submit_command_buffer(cmd_buf);
        } else {
            log::error!("ShadowAgent: encoder.finish() returned None — skipping shadow submission");
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
                "shadow_strategy={:?} time={measured_time_ms:.2}ms",
                self.strategy
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
        lanes.register(Box::new(StandardShadowsLane::default()));
        lanes.register(Box::new(MediumShadowsLane::default()));
        lanes.register(Box::new(LowResShadowsLane::default()));

        Self {
            lanes,
            // Default to full quality; GORNA can step down to Budget on
            // pressure via `apply_budget`.
            strategy: ShadowStrategy::Standard,
            time_budget: Duration::ZERO,
            current_strategy: StrategyId::HighPerformance,
            frame_status: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::control::gorna::ResourceBudget;
    use std::collections::HashMap;

    fn budget(strategy_id: StrategyId) -> ResourceBudget {
        ResourceBudget {
            strategy_id,
            time_limit: Duration::from_millis(8),
            memory_limit: None,
            extra_params: HashMap::new(),
        }
    }

    #[test]
    fn from_strategy_id_maps_low_power_to_low_res() {
        assert_eq!(
            ShadowStrategy::from_strategy_id(StrategyId::LowPower),
            ShadowStrategy::LowRes
        );
    }

    #[test]
    fn from_strategy_id_maps_each_tier_to_a_distinct_strategy() {
        assert_eq!(
            ShadowStrategy::from_strategy_id(StrategyId::HighPerformance),
            ShadowStrategy::Standard
        );
        assert_eq!(
            ShadowStrategy::from_strategy_id(StrategyId::Balanced),
            ShadowStrategy::Medium
        );
        assert_eq!(
            ShadowStrategy::from_strategy_id(StrategyId::LowPower),
            ShadowStrategy::LowRes
        );
    }

    #[test]
    fn apply_budget_low_power_selects_low_res_lane() {
        let mut agent = ShadowAgent::default();
        agent.apply_budget(budget(StrategyId::LowPower));
        assert_eq!(agent.strategy, ShadowStrategy::LowRes);
        assert_eq!(agent.strategy.lane_name(), LOW_RES_STRATEGY_NAME);
        assert_eq!(agent.report_status().current_strategy, StrategyId::LowPower);
    }

    #[test]
    fn apply_budget_high_performance_selects_standard_lane() {
        let mut agent = ShadowAgent::default();
        agent.apply_budget(budget(StrategyId::HighPerformance));
        assert_eq!(agent.strategy, ShadowStrategy::Standard);
        assert_eq!(agent.strategy.lane_name(), STANDARD_STRATEGY_NAME);
        assert_eq!(
            agent.report_status().current_strategy,
            StrategyId::HighPerformance
        );
    }

    #[test]
    fn apply_budget_balanced_selects_medium_lane() {
        let mut agent = ShadowAgent::default();
        agent.apply_budget(budget(StrategyId::Balanced));
        assert_eq!(agent.strategy, ShadowStrategy::Medium);
        assert_eq!(agent.strategy.lane_name(), MEDIUM_STRATEGY_NAME);
    }

    #[test]
    fn registered_lanes_match_strategy_names() {
        let agent = ShadowAgent::default();
        assert!(agent.lanes.get(STANDARD_STRATEGY_NAME).is_some());
        assert!(agent.lanes.get(MEDIUM_STRATEGY_NAME).is_some());
        assert!(agent.lanes.get(LOW_RES_STRATEGY_NAME).is_some());
    }

    #[test]
    fn negotiate_offers_three_distinct_tiers() {
        let mut agent = ShadowAgent::default();
        let response = agent.negotiate(NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: Default::default(),
            current_mode: khora_core::agent::mode::EngineMode::Playing,
            agent_timing: agent.execution_timing(),
        });

        let ids: Vec<StrategyId> = response.strategies.iter().map(|s| s.id).collect();
        assert!(ids.contains(&StrategyId::HighPerformance));
        assert!(ids.contains(&StrategyId::Balanced));
        assert!(ids.contains(&StrategyId::LowPower));

        // VRAM quotes must be strictly decreasing across the tiers — three
        // genuinely different pipelines, not relabeled copies.
        let vram = |id: StrategyId| {
            response
                .strategies
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.estimated_vram)
                .unwrap()
        };
        assert!(vram(StrategyId::HighPerformance) > vram(StrategyId::Balanced));
        assert!(vram(StrategyId::Balanced) > vram(StrategyId::LowPower));
        assert!(
            vram(StrategyId::LowPower) > 0,
            "LowRes still has real atlases"
        );
    }

    #[test]
    fn negotiate_under_vram_pressure_drops_expensive_tiers() {
        let mut agent = ShadowAgent::default();
        let response = agent.negotiate(NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: khora_core::control::gorna::ResourceConstraints {
                // Below Medium's 22 MiB but above LowRes's ~5.5 MiB.
                max_vram_bytes: Some(8 * 1024 * 1024),
                ..Default::default()
            },
            current_mode: khora_core::agent::mode::EngineMode::Playing,
            agent_timing: agent.execution_timing(),
        });

        let ids: Vec<StrategyId> = response.strategies.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![StrategyId::LowPower]);
    }
}
