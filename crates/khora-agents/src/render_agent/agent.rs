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

//! Defines the RenderAgent — owns `LaneKind::Render` lanes only.
//!
//! Per CLAD, an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA/strategy state.  Everything else (the graphics device, the
//! render system, the GPU mesh cache, the per-frame `RenderWorld`) is
//! looked up from the [`ServiceRegistry`] each frame — agents are not
//! the owners of those resources.

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use khora_core::agent::{
    Agent, AgentDependency, AgentImportance, DependencyKind, ExecutionPhase, ExecutionTiming,
};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::{
    ClearColor, ColorTarget, DepthTarget, LaneContext, LaneKind, LaneRegistry, ShadowAtlasView,
    ShadowComparisonSampler, Slot,
};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::{GraphicsDevice, RenderSystem};
use khora_core::EngineContext;
use khora_data::assets::Assets;
use khora_data::ecs::World;
use khora_data::render::{
    extract_active_camera_view, PassDescriptor, RenderWorld, RenderWorldStore, ResourceId,
    SharedFrameGraph,
};
use khora_data::GpuCache;
use khora_lanes::render_lane::{ForwardPlusLane, LitForwardLane, SimpleUnlitLane};

/// Threshold for switching to Forward+ rendering.
const FORWARD_PLUS_LIGHT_THRESHOLD: usize = 20;

/// Scale factor converting lane cost units to milliseconds of GPU time.
const COST_TO_MS_SCALE: f32 = 5.0;

/// Approximate VRAM per mesh in bytes (vertex + index buffers).
const DEFAULT_VRAM_PER_MESH: u64 = 100 * 1024;

/// Rendering strategy selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderingStrategy {
    /// Simple unlit rendering (vertex colors only).
    #[default]
    Unlit,
    /// Standard forward rendering with lighting.
    LitForward,
    /// Forward+ (tiled forward) rendering with compute-based light culling.
    ForwardPlus,
    /// Automatic selection based on scene complexity (light count).
    Auto,
}

/// The agent responsible for the main render pass (`LaneKind::Render`).
///
/// Holds **only** its own strategy state — every other dependency
/// (`GraphicsDevice`, `RenderSystem`, `GpuCache`, `RenderWorldStore`,
/// `FrameContext`) is fetched from `EngineContext::services` per frame.
pub struct RenderAgent {
    /// Render lanes — the agent's strategies.
    lanes: LaneRegistry,
    /// Current rendering strategy selection mode.
    strategy: RenderingStrategy,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Duration of the last `execute` call.
    last_frame_time: Duration,
    /// Number of draw calls issued in the last frame.
    draw_call_count: u32,
    /// Number of triangles rendered in the last frame.
    triangle_count: u32,
    /// Total number of frames rendered since agent creation.
    frame_count: u64,
    /// Number of lights in the most recently extracted scene (for status).
    last_light_count: usize,
    /// Number of `execute` invocations attempted.  Used by `is_stalled` to
    /// distinguish "never tried" from "tried but produced no frame".
    execute_attempts: u64,
}

impl Agent for RenderAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        let mut strategies = Vec::new();
        // Negotiate from a stub LaneContext: we don't have access to the live
        // RenderWorld here (negotiate runs on the DCC thread), so we estimate
        // costs against the lane defaults.
        let mut stub_world = RenderWorld::new();
        let mut ctx = LaneContext::new();
        ctx.insert(Slot::new(&mut stub_world));

        for lane in self.lanes.find_by_kind(LaneKind::Render) {
            let cost = lane.estimate_cost(&ctx);
            let estimated_time =
                Duration::from_secs_f32((cost * COST_TO_MS_SCALE).max(0.1) / 1000.0);

            let (strategy_id, vram_overhead) = match lane.strategy_name() {
                "SimpleUnlit" => (StrategyId::LowPower, 0u64),
                "LitForward" => (StrategyId::Balanced, 4096u64),
                "ForwardPlus" => (StrategyId::HighPerformance, 4096 + 8 * 1024 * 1024),
                _ => continue,
            };

            // Without a populated RenderWorld we can only quote VRAM at the
            // lane-overhead level; per-mesh VRAM is folded in by the lane's
            // own cost estimator at execute time when it has the real scene.
            let estimated_vram = vram_overhead;

            if let Some(max_vram) = request.constraints.max_vram_bytes {
                if estimated_vram > max_vram {
                    continue;
                }
            }

            strategies.push(StrategyOption {
                id: strategy_id,
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

        // The mesh-count component of VRAM is included implicitly — agents
        // no longer own the RenderWorld so we cannot compute it here.  The
        // arbiter accepts these as worst-case-overhead estimates.
        let _ = DEFAULT_VRAM_PER_MESH;

        NegotiationResponse {
            strategies,
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!(
            "RenderAgent: Strategy update to {:?} (time_limit={:?})",
            budget.strategy_id,
            budget.time_limit,
        );

        match budget.strategy_id {
            StrategyId::LowPower => self.strategy = RenderingStrategy::Auto,
            StrategyId::Balanced => self.strategy = RenderingStrategy::LitForward,
            StrategyId::HighPerformance => self.strategy = RenderingStrategy::ForwardPlus,
            StrategyId::Custom(_) => {
                log::warn!(
                    "RenderAgent received unsupported custom strategy. Falling back to Balanced."
                );
                self.strategy = RenderingStrategy::LitForward;
            }
        }

        self.current_strategy = budget.strategy_id;
        self.time_budget = budget.time_limit;
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        // One-shot lane GPU initialization.  We fetch the device from the
        // service registry, drive lane.on_initialize() once, and drop the
        // device handle — the agent does not store it.
        let Some(device_arc) = context.services.get::<Arc<dyn GraphicsDevice>>().cloned() else {
            log::warn!("RenderAgent: graphics device unavailable in on_initialize");
            return;
        };

        let mut init_ctx = LaneContext::new();
        init_ctx.insert(device_arc);
        for lane in self.lanes.all() {
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!(
                    "RenderAgent: Failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        self.execute_attempts += 1;

        // Look up every dependency from services — the agent owns none of these.
        let Some(device_arc) = context.services.get::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        let Some(rs_arc) = context.services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>() else {
            return;
        };
        let render_system: Arc<Mutex<Box<dyn RenderSystem>>> = (*rs_arc).clone();

        let Some(gpu_cache) = context.services.get::<GpuCache>() else {
            return;
        };
        let gpu_meshes: Arc<RwLock<Assets<GpuMesh>>> = gpu_cache.inner().clone();

        let Some(rws) = context.services.get::<RenderWorldStore>() else {
            return;
        };
        let render_world_arc: Arc<RwLock<RenderWorld>> = rws.shared().clone();

        let Some(frame_graph) = context.services.get::<SharedFrameGraph>().cloned() else {
            log::warn!("RenderAgent: no FrameGraph in services");
            return;
        };

        // The engine inserts ColorTarget/DepthTarget/ClearColor and the shadow
        // atlas data into the per-frame FrameContext after `begin_frame()`.
        // ShadowAgent runs in OBSERVE (before OUTPUT) and publishes its atlas
        // there as well. We just read both.
        let Some(fctx) = context.services.get::<Arc<FrameContext>>() else {
            log::warn!("RenderAgent: no FrameContext in services");
            return;
        };
        let Some(color_target) = fctx.get::<ColorTarget>().map(|a| *a) else {
            log::warn!("RenderAgent: ColorTarget missing in FrameContext");
            return;
        };
        let depth_target = fctx.get::<DepthTarget>().map(|a| *a);
        let clear_color = fctx.get::<ClearColor>().map(|a| *a).unwrap_or_else(|| {
            ClearColor(khora_core::math::LinearRgba::new(0.1, 0.1, 0.15, 1.0))
        });
        let shadow_atlas = fctx.get::<ShadowAtlasView>().map(|a| *a);
        let shadow_sampler = fctx.get::<ShadowComparisonSampler>().map(|a| *a);

        // Push the active camera view into the render system if present.
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                if let Some(view_info) = extract_active_camera_view(world) {
                    if let Ok(mut rs) = render_system.lock() {
                        rs.prepare_frame(&view_info);
                    }
                }
            }
        }

        let frame_start = Instant::now();
        let strategy = self.strategy;
        let select_name = {
            let rw = render_world_arc.read().unwrap();
            lane_name_for_strategy(strategy, &rw)
        };

        // Encode the scene pass into a fresh command buffer; the FrameGraph
        // submits it once all agents have finished recording.
        let mut encoder = device.create_command_encoder(Some("Khora Scene Encoder"));
        {
            let mut rw_guard = render_world_arc.write().unwrap();
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            ctx.insert(gpu_meshes.clone());
            // SAFETY: encoder is alive for this whole block; ctx (which holds
            // the slot) is dropped before encoder.finish() consumes it.
            let encoder_slot = Slot::new(encoder.as_mut());
            ctx.insert(unsafe {
                std::mem::transmute::<
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                    Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                >(encoder_slot)
            });
            ctx.insert(Slot::new(&mut *rw_guard));
            ctx.insert(color_target);
            if let Some(dt) = depth_target {
                ctx.insert(dt);
            }
            ctx.insert(clear_color);
            if let Some(view) = shadow_atlas {
                ctx.insert(view);
            }
            if let Some(sampler) = shadow_sampler {
                ctx.insert(sampler);
            }

            if let Some(lane) = self.lanes.get(select_name) {
                if let Err(e) = lane.execute(&mut ctx) {
                    log::error!("Render lane {} failed: {}", lane.strategy_name(), e);
                }
            }
        }
        let cmd_buf = encoder.finish();

        let mut descriptor = PassDescriptor::new("ScenePass")
            .writes(ResourceId::Color)
            .writes(ResourceId::Depth);
        if shadow_atlas.is_some() {
            descriptor = descriptor.reads(ResourceId::ShadowAtlas);
        }
        frame_graph
            .lock()
            .expect("FrameGraph mutex poisoned")
            .add_pass(descriptor, cmd_buf);

        self.last_frame_time = frame_start.elapsed();

        // Refresh per-frame metrics from the populated RenderWorld.
        let rw_guard = render_world_arc.read().unwrap();
        self.draw_call_count = rw_guard.meshes.len() as u32;
        self.triangle_count = count_triangles(&rw_guard, &gpu_meshes);
        self.last_light_count = rw_guard.directional_light_count()
            + rw_guard.point_light_count()
            + rw_guard.spot_light_count();
        drop(rw_guard);

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
                "frame_time={:.2}ms draws={} tris={} lights={}",
                self.last_frame_time.as_secs_f32() * 1000.0,
                self.draw_call_count,
                self.triangle_count,
                self.last_light_count,
            ),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            priority: 1.0,
            importance: AgentImportance::Critical,
            fixed_timestep: None,
            // ShadowAgent (in OBSERVE) publishes the shadow atlas into the
            // FrameContext that this agent reads. The scheduler uses this
            // declaration to enforce ordering via the AgentCompletionMap.
            dependencies: vec![AgentDependency {
                target: AgentId::ShadowRenderer,
                kind: DependencyKind::Hard,
                condition: None,
            }],
        }
    }
}

impl Default for RenderAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(SimpleUnlitLane::new()));
        lanes.register(Box::new(LitForwardLane::new()));
        lanes.register(Box::new(ForwardPlusLane::new()));

        Self {
            lanes,
            strategy: RenderingStrategy::Auto,
            current_strategy: StrategyId::Balanced,
            time_budget: Duration::ZERO,
            last_frame_time: Duration::ZERO,
            draw_call_count: 0,
            triangle_count: 0,
            frame_count: 0,
            last_light_count: 0,
            execute_attempts: 0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Free helpers — kept off the agent struct per CLAD trait-purity rule.
// ─────────────────────────────────────────────────────────────────────

fn lane_name_for_strategy(strategy: RenderingStrategy, world: &RenderWorld) -> &'static str {
    match strategy {
        RenderingStrategy::Unlit => "SimpleUnlit",
        RenderingStrategy::LitForward => "LitForward",
        RenderingStrategy::ForwardPlus => "ForwardPlus",
        RenderingStrategy::Auto => {
            let total_lights = world.directional_light_count()
                + world.point_light_count()
                + world.spot_light_count();
            if total_lights > FORWARD_PLUS_LIGHT_THRESHOLD {
                "ForwardPlus"
            } else if total_lights > 0 {
                "LitForward"
            } else {
                "SimpleUnlit"
            }
        }
    }
}

fn count_triangles(render_world: &RenderWorld, gpu_meshes: &RwLock<Assets<GpuMesh>>) -> u32 {
    use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;

    let Ok(guard) = gpu_meshes.read() else {
        return 0;
    };
    let mut total = 0u32;
    for mesh in &render_world.meshes {
        if let Some(gpu_mesh) = guard.get(&mesh.cpu_mesh_uuid) {
            total += match gpu_mesh.primitive_topology {
                PrimitiveTopology::TriangleList => gpu_mesh.index_count / 3,
                PrimitiveTopology::TriangleStrip => gpu_mesh.index_count.saturating_sub(2),
                _ => 0,
            };
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::agent::{EngineMode, ExecutionTiming};
    use khora_core::control::gorna::{NegotiationRequest, ResourceConstraints, StrategyId};

    #[test]
    fn test_negotiate_offers_three_default_strategies() {
        let mut agent = RenderAgent::default();
        let req = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: ResourceConstraints::default(),
            current_mode: EngineMode::Playing,
            agent_timing: ExecutionTiming::default(),
        };
        let res = agent.negotiate(req);
        assert_eq!(res.strategies.len(), 3);
    }

    #[test]
    fn test_negotiate_vram_constrained_returns_only_low_power() {
        let mut agent = RenderAgent::default();
        let req = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: ResourceConstraints {
                max_vram_bytes: Some(10),
                ..Default::default()
            },
            current_mode: EngineMode::Playing,
            agent_timing: ExecutionTiming::default(),
        };
        let res = agent.negotiate(req);
        assert_eq!(res.strategies.len(), 1);
        assert_eq!(res.strategies[0].id, StrategyId::LowPower);
    }

    #[test]
    fn test_apply_budget_records_strategy_in_status() {
        let mut agent = RenderAgent::default();
        agent.apply_budget(ResourceBudget {
            strategy_id: StrategyId::HighPerformance,
            time_limit: Duration::from_millis(12),
            memory_limit: None,
            extra_params: std::collections::HashMap::new(),
        });
        assert_eq!(
            agent.report_status().current_strategy,
            StrategyId::HighPerformance
        );
    }

    #[test]
    fn test_report_status_initial_state() {
        let agent = RenderAgent::default();
        let status = agent.report_status();
        assert_eq!(status.agent_id, AgentId::Renderer);
        assert_eq!(status.health_score, 1.0);
        assert!(!status.is_stalled);
    }
}
