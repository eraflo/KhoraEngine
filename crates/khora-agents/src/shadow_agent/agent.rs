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

use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::lane::{Lane, LaneContext, LaneKind, Slot};
use khora_core::renderer::{GraphicsDevice, RenderSystem};
use khora_core::EngineContext;
use khora_data::{
    assets::Assets,
    ecs::World,
};
use khora_lanes::render_lane::{ExtractRenderablesLane, RenderWorld, ShadowPassLane};
use crate::render_agent::mesh_preparation::MeshPreparationSystem;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Marker type for shadow completion stage.
pub struct ShadowDone;

/// The agent responsible for shadow map rendering.
pub struct ShadowAgent {
    /// Intermediate data structure populated by the extraction phase.
    render_world: RenderWorld,
    /// Cache for GPU-side mesh assets.
    gpu_meshes: Arc<RwLock<Assets<khora_core::renderer::api::scene::GpuMesh>>>,
    /// System that handles uploading CPU meshes to the GPU.
    mesh_preparation_system: MeshPreparationSystem,
    /// Lane that extracts data from the ECS into the RenderWorld.
    extract_lane: ExtractRenderablesLane,
    /// Shadow processing lanes.
    shadow_lanes: Vec<Box<dyn Lane>>,
    /// Cached reference to the graphics device.
    device: Option<Arc<dyn GraphicsDevice>>,
    /// Cached reference to the render system.
    render_system: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    /// Performance metrics.
    last_frame_time: Duration,
    time_budget: Duration,
    frame_count: u64,
}

impl ShadowAgent {
    /// Creates a new ShadowAgent with initialized shadow lanes.
    pub fn new() -> Self {
        let shadow_lanes: Vec<Box<dyn Lane>> =
            vec![Box::new(ShadowPassLane::default())];

        Self {
            render_world: RenderWorld::new(),
            gpu_meshes: Arc::new(RwLock::new(Assets::new())),
            mesh_preparation_system: MeshPreparationSystem::new(Arc::new(RwLock::new(Assets::new()))),
            extract_lane: ExtractRenderablesLane::new(),
            shadow_lanes,
            device: None,
            render_system: None,
            last_frame_time: Duration::ZERO,
            time_budget: Duration::from_millis(5),
            frame_count: 0,
        }
    }
}

impl Agent for ShadowAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, _budget: ResourceBudget) {}

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        if let Some(device) = context
            .services
            .get::<Arc<dyn GraphicsDevice>>()
            .cloned()
        {
            self.device = Some(device.clone());

            let device_arc = device.clone();
            let mut init_ctx = LaneContext::new();
            init_ctx.insert(device_arc);
            for lane in &self.shadow_lanes {
                if let Err(e) = lane.on_initialize(&mut init_ctx) {
                    log::error!(
                        "ShadowAgent: Failed to initialize lane {}: {}",
                        lane.strategy_name(),
                        e
                    );
                }
            }
        }

        if self.render_system.is_none() {
            if let Some(rs) = context
                .services
                .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            {
                self.render_system = Some(rs.clone());
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        if self.render_system.is_none() {
            if let Some(rs) = context
                .services
                .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            {
                self.render_system = Some(rs.clone());
            }
        }

        let Some(device) = self.device.clone() else {
            return;
        };

        let Some(rs) = &self.render_system else {
            return;
        };

        // Prepare mesh assets and extract render world.
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                self.render_world.clear();
                self.mesh_preparation_system
                    .run(world, device.as_ref());

                let world_ptr: *mut World = world;
                let render_world_ptr: *mut RenderWorld = &mut self.render_world;
                let gpu_meshes_ptr: *const Arc<RwLock<Assets<khora_core::renderer::api::scene::GpuMesh>>> = &self.gpu_meshes;

                let mut ctx = LaneContext::new();
                ctx.insert(Slot::new(unsafe { &mut *render_world_ptr }));
                ctx.insert(unsafe { (*gpu_meshes_ptr).clone() });
                ctx.insert(unsafe { &mut *world_ptr });
                if let Err(e) = self.extract_lane.execute(&mut ctx) {
                    log::warn!("ShadowAgent: extract lane failed: {}", e);
                }
            }
        }

        // Render shadow maps synchronously.
        if let Ok(mut rs) = rs.lock() {
            if let Err(e) = rs.render_with_encoder(
                khora_core::math::LinearRgba::new(0.0, 0.0, 0.0, 0.0),
                Box::new(|encoder, _render_ctx| {
                    let mut ctx = LaneContext::new();
                    ctx.insert(device.clone());
                    let encoder_slot = Slot::new(encoder);
                    ctx.insert(unsafe {
                        std::mem::transmute::<
                            Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                            Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                        >(encoder_slot)
                    });
                    ctx.insert(Slot::new(&mut self.render_world));

                    for shadow_lane in &self.shadow_lanes {
                        if shadow_lane.lane_kind() == LaneKind::Shadow {
                            if let Err(e) = shadow_lane.execute(&mut ctx) {
                                log::error!(
                                    "ShadowAgent: Shadow lane {} failed: {}",
                                    shadow_lane.strategy_name(),
                                    e
                                );
                            }
                        }
                    }
                }),
            ) {
                log::error!("ShadowAgent: Shadow render error: {}", e);
            }
        }

        // Signal shadow completion via StageHandle for other agents.
        if let Some(fctx) = context
            .services
            .get::<Arc<khora_core::renderer::api::core::FrameContext>>()
        {
            let stage = fctx.insert_stage::<ShadowDone>();
            stage.mark_done();
        }

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
            current_strategy: StrategyId::LowPower,
            is_stalled: self.frame_count == 0 && self.device.is_some(),
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
