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

//! Defines the RenderAgent, the central orchestrator for the rendering subsystem.

use super::mesh_preparation::MeshPreparationSystem;
use khora_core::lane::{ClearColor, ColorTarget, DepthTarget};
use khora_core::{
    agent::Agent,
    control::gorna::{
        AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyOption,
    },
    lane::{Lane, LaneContext, LaneKind, LaneRegistry, Slot},
    math::Mat4,
    renderer::{
        api::{
            resource::ViewInfo,
            scene::{GpuMesh, RenderObject},
        },
        GraphicsDevice, RenderSystem,
    },
    EngineContext,
};
use khora_data::{
    assets::Assets,
    ecs::{Camera, GlobalTransform, World},
};
use khora_lanes::render_lane::{
    ExtractRenderablesLane, ForwardPlusLane, LitForwardLane, RenderWorld, ShadowPassLane,
    SimpleUnlitLane,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use khora_core::control::gorna::{AgentId, StrategyId};
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::telemetry::event::TelemetryEvent;
use khora_core::telemetry::monitoring::GpuReport;

/// Threshold for switching to Forward+ rendering.
/// When the scene has more than this many lights, Forward+ is preferred.
const FORWARD_PLUS_LIGHT_THRESHOLD: usize = 20;

/// Scale factor converting lane cost units to milliseconds of GPU time.
/// Used by `negotiate()` to provide realistic time estimates to GORNA.
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

/// The agent responsible for managing the state and logic of the rendering pipeline.
pub struct RenderAgent {
    // Intermediate data structure populated by the extraction phase.
    render_world: RenderWorld,
    // Cache for GPU-side mesh assets (shared via Arc with MeshPreparationSystem).
    gpu_meshes: Arc<RwLock<Assets<GpuMesh>>>,
    // System that handles uploading CPU meshes to the GPU.
    mesh_preparation_system: MeshPreparationSystem,
    // Lane that extracts data from the ECS into the RenderWorld.
    extract_lane: ExtractRenderablesLane,
    // All processing lanes (render + shadow) stored generically.
    lanes: LaneRegistry,
    // Current rendering strategy selection mode.
    strategy: RenderingStrategy,
    // Current active strategy ID from negotiation.
    current_strategy: StrategyId,
    // Cached reference to the graphics device for lane lifecycle management.
    device: Option<Arc<dyn GraphicsDevice>>,
    // Cached reference to the render system (obtained from ServiceRegistry in update()).
    render_system: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    // Optional channel to emit telemetry events to the DCC.
    telemetry_sender: Option<Sender<TelemetryEvent>>,
    // --- Performance Metrics (for GORNA health reporting) ---
    // Duration of the last render() call.
    last_frame_time: Duration,
    // Time budget assigned by GORNA via apply_budget().
    time_budget: Duration,
    // Number of draw calls issued in the last frame.
    draw_call_count: u32,
    // Number of triangles rendered in the last frame.
    triangle_count: u32,
    // Total number of frames rendered since agent creation.
    frame_count: u64,
}

impl Agent for RenderAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        let mut strategies = Vec::new();
        let mesh_count = self.render_world.meshes.len() as u64;
        let base_vram = mesh_count * DEFAULT_VRAM_PER_MESH;

        // Build a minimal context for cost estimation.
        let mut ctx = LaneContext::new();
        ctx.insert(Slot::new(&mut self.render_world));
        ctx.insert(self.gpu_meshes.clone());

        // Build strategy options from each available render lane using actual cost data.
        for lane in self.lanes.find_by_kind(LaneKind::Render) {
            let cost = lane.estimate_cost(&ctx);
            let estimated_time =
                Duration::from_secs_f32((cost * COST_TO_MS_SCALE).max(0.1) / 1000.0);

            let (strategy_id, vram_overhead) = match lane.strategy_name() {
                "SimpleUnlit" => (StrategyId::LowPower, 0u64),
                "LitForward" => {
                    // Uniform buffers: ~512B per mesh + ~4KB global uniforms.
                    (StrategyId::Balanced, mesh_count * 512 + 4096)
                }
                "ForwardPlus" => {
                    // LitForward overhead + ~8MB compute buffers for light culling.
                    (
                        StrategyId::HighPerformance,
                        mesh_count * 512 + 4096 + 8 * 1024 * 1024,
                    )
                }
                _ => continue,
            };

            let estimated_vram = base_vram + vram_overhead;

            // Respect VRAM constraints from the negotiation request.
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

        // Always guarantee at least the LowPower fallback.
        if strategies.is_empty() {
            strategies.push(StrategyOption {
                id: StrategyId::LowPower,
                estimated_time: Duration::from_millis(1),
                estimated_vram: base_vram,
            });
        }

        NegotiationResponse { strategies }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!(
            "RenderAgent: Strategy update to {:?} (time_limit={:?})",
            budget.strategy_id,
            budget.time_limit,
        );

        // Map the GORNA strategy to our internal rendering strategy.
        // Lanes remain alive — we only switch which one is active.
        // LowPower maps to Auto — this lets the agent use Unlit when there are
        // no lights, but automatically switch to LitForward when the scene
        // contains lights so that shadows and lighting work correctly.
        match budget.strategy_id {
            StrategyId::LowPower => {
                self.strategy = RenderingStrategy::Auto;
            }
            StrategyId::Balanced => {
                self.strategy = RenderingStrategy::LitForward;
            }
            StrategyId::HighPerformance => {
                self.strategy = RenderingStrategy::ForwardPlus;
            }
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

    fn update(&mut self, context: &mut EngineContext<'_>) {
        // Cache the graphics device from the service registry.
        if self.device.is_none() {
            if let Some(device_arc) = context.services.get::<Arc<dyn GraphicsDevice>>() {
                self.device = Some(device_arc.clone());

                // Initialize all lanes via Lane abstraction.
                let mut init_ctx = LaneContext::new();
                init_ctx.insert(device_arc.clone());
                for lane in self.lanes.all() {
                    if let Err(e) = lane.on_initialize(&mut init_ctx) {
                        log::error!("Failed to initialize lane {}: {}", lane.strategy_name(), e);
                    }
                }
            }
        }

        // Cache the render system from the service registry.
        if self.render_system.is_none() {
            if let Some(rs) = context.services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>() {
                self.render_system = Some(rs.clone());
            }
        }

        let Some(device) = self.device.clone() else {
            return;
        };

        // Step 1: Extract scene data from ECS into RenderWorld.
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                // Access the CPU mesh assets and material assets.
                self.prepare_frame(world, device.as_ref());

                // Step 2: Extract camera view and push to RenderSystem.
                let view_info = self.extract_camera_view(world);
                if let Some(rs) = &self.render_system {
                    if let Ok(mut rs) = rs.lock() {
                        rs.prepare_frame(&view_info);
                    }
                }
            }
        }

        // Step 3: Render — call render_with_encoder on the cached render system.
        //
        // The closure builds a LaneContext and executes lanes through the Lane abstraction:
        //   1. Shadow lanes: encode depth-only draw calls, patch lights, store shadow resources
        //   2. Main pass: render scene with shadow data
        if let Some(rs) = self.render_system.clone() {
            if let Ok(mut rs) = rs.lock() {
                let clear_color = khora_core::math::LinearRgba::new(0.1, 0.1, 0.15, 1.0);
                let selected_name = self.select_lane_name();

                let render_world = &mut self.render_world;
                let gpu_meshes = &self.gpu_meshes;
                let lanes = &self.lanes;

                let frame_start = Instant::now();

                match rs.render_with_encoder(
                    clear_color,
                    Box::new(|encoder, render_ctx| {
                        let mut ctx = LaneContext::new();
                        ctx.insert(device.clone());
                        ctx.insert(gpu_meshes.clone());
                        // SAFETY: encoder lives for the entirety of this closure.
                        // ctx is created and consumed within the same closure scope.
                        // transmute erases the trait object lifetime ('1 → 'static)
                        // which is safe because the data outlives the Slot.
                        let encoder_slot = Slot::new(encoder);
                        ctx.insert(unsafe {
                            std::mem::transmute::<
                                Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                                Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                            >(encoder_slot)
                        });
                        ctx.insert(Slot::new(render_world));
                        ctx.insert(ColorTarget(*render_ctx.color_target));
                        if let Some(dt) = render_ctx.depth_target {
                            ctx.insert(DepthTarget(*dt));
                        }
                        ctx.insert(ClearColor(render_ctx.clear_color));

                        // 1. Execute shadow lanes (they insert ShadowAtlasView + ShadowComparisonSampler)
                        for shadow_lane in lanes.find_by_kind(LaneKind::Shadow) {
                            if let Err(e) = shadow_lane.execute(&mut ctx) {
                                log::error!(
                                    "Shadow lane {} failed: {}",
                                    shadow_lane.strategy_name(),
                                    e
                                );
                            }
                        }

                        // 2. Execute selected render lane
                        if let Some(lane) = lanes.get(selected_name) {
                            if let Err(e) = lane.execute(&mut ctx) {
                                log::error!("Render lane {} failed: {}", lane.strategy_name(), e);
                            }
                        }
                    }),
                ) {
                    Ok(_stats) => {
                        log::trace!("RenderAgent: Frame rendered successfully.");
                    }
                    Err(e) => log::error!("RenderAgent: Render error: {}", e),
                }

                self.last_frame_time = frame_start.elapsed();
            }
        }

        // Update frame metrics.
        self.draw_call_count = self.render_world.meshes.len() as u32;
        self.triangle_count = self.count_triangles();
        self.frame_count += 1;

        // Emit telemetry to the DCC if a sender is wired.
        self.emit_telemetry();
    }

    fn report_status(&self) -> AgentStatus {
        let health_score = if self.time_budget.is_zero() || self.frame_count == 0 {
            // No budget assigned yet or no frames rendered — report healthy.
            1.0
        } else {
            // Health = how well we fit within the GORNA time budget.
            // 1.0 = at or under budget, <1.0 = over budget.
            let ratio =
                self.time_budget.as_secs_f32() / self.last_frame_time.as_secs_f32().max(0.0001);
            ratio.min(1.0)
        };

        let total_lights = self.render_world.directional_light_count()
            + self.render_world.point_light_count()
            + self.render_world.spot_light_count();

        AgentStatus {
            agent_id: self.id(),
            health_score,
            current_strategy: self.current_strategy,
            is_stalled: self.frame_count == 0 && self.device.is_some(),
            message: format!(
                "frame_time={:.2}ms draws={} tris={} lights={}",
                self.last_frame_time.as_secs_f32() * 1000.0,
                self.draw_call_count,
                self.triangle_count,
                total_lights,
            ),
        }
    }

    fn execute(&mut self) {
        // RenderAgent doesn't do anything in generic execute()
        // Use render() method for actual rendering with encoder
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl RenderAgent {
    /// Creates a new `RenderAgent` with default lanes and automatic strategy selection.
    pub fn new() -> Self {
        let gpu_meshes = Arc::new(RwLock::new(Assets::new()));

        // Register all default lanes (render + shadow) generically.
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(SimpleUnlitLane::new()));
        lanes.register(Box::new(LitForwardLane::new()));
        lanes.register(Box::new(ForwardPlusLane::new()));
        lanes.register(Box::new(ShadowPassLane::new()));

        Self {
            render_world: RenderWorld::new(),
            gpu_meshes: gpu_meshes.clone(),
            mesh_preparation_system: MeshPreparationSystem::new(gpu_meshes),
            extract_lane: ExtractRenderablesLane::new(),
            lanes,
            strategy: RenderingStrategy::Auto,
            current_strategy: StrategyId::Balanced,
            device: None,
            render_system: None,
            telemetry_sender: None,
            last_frame_time: Duration::ZERO,
            time_budget: Duration::ZERO,
            draw_call_count: 0,
            triangle_count: 0,
            frame_count: 0,
        }
    }

    /// Renders the scene using the provided encoder and render context.
    ///
    /// This is the main rendering method that encodes GPU commands via the selected lane.
    /// Shadow lanes are executed first, followed by the selected render lane.
    pub fn render(
        &mut self,
        encoder: &mut dyn khora_core::renderer::traits::CommandEncoder,
        render_ctx: &khora_core::renderer::api::core::RenderContext,
    ) {
        let frame_start = Instant::now();

        let Some(device) = self.device.clone() else {
            return;
        };

        self.draw_call_count = self.render_world.meshes.len() as u32;
        self.triangle_count = self.count_triangles();

        // Build LaneContext with all required data.
        let mut ctx = LaneContext::new();
        ctx.insert(device);
        ctx.insert(self.gpu_meshes.clone());
        // SAFETY: encoder lives for the entirety of this method call.
        // ctx is stack-scoped and dropped before returning.
        // transmute erases the trait object lifetime ('a → 'static).
        let encoder_slot = Slot::new(encoder);
        ctx.insert(unsafe {
            std::mem::transmute::<
                Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                Slot<dyn khora_core::renderer::traits::CommandEncoder>,
            >(encoder_slot)
        });
        ctx.insert(Slot::new(&mut self.render_world));
        ctx.insert(ColorTarget(*render_ctx.color_target));
        if let Some(dt) = render_ctx.depth_target {
            ctx.insert(DepthTarget(*dt));
        }
        ctx.insert(ClearColor(render_ctx.clear_color));

        // 1. Execute shadow lanes (they insert ShadowAtlasView + ShadowComparisonSampler)
        for shadow_lane in self.lanes.find_by_kind(LaneKind::Shadow) {
            if let Err(e) = shadow_lane.execute(&mut ctx) {
                log::error!("Shadow lane {} failed: {}", shadow_lane.strategy_name(), e);
            }
        }

        // 2. Execute selected render lane
        let selected_name = self.select_lane_name();
        if let Some(lane) = self.lanes.get(selected_name) {
            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("Render lane {} failed: {}", lane.strategy_name(), e);
            }
        }

        self.last_frame_time = frame_start.elapsed();
        self.frame_count += 1;
    }

    /// Creates a new `RenderAgent` with the specified rendering strategy.
    pub fn with_strategy(strategy: RenderingStrategy) -> Self {
        let mut agent = Self::new();
        agent.strategy = strategy;
        agent
    }

    /// Attaches a telemetry sender so the agent can emit `GpuReport` events to the DCC.
    pub fn with_telemetry_sender(mut self, sender: Sender<TelemetryEvent>) -> Self {
        self.telemetry_sender = Some(sender);
        self
    }

    /// Adds a custom lane to the registry.
    pub fn add_lane(&mut self, lane: Box<dyn Lane>) {
        self.lanes.register(lane);
    }

    /// Sets the rendering strategy.
    pub fn set_strategy(&mut self, strategy: RenderingStrategy) {
        self.strategy = strategy;
    }

    /// Returns the current rendering strategy.
    pub fn strategy(&self) -> RenderingStrategy {
        self.strategy
    }

    /// Returns a reference to the lane registry.
    pub fn lanes(&self) -> &LaneRegistry {
        &self.lanes
    }

    /// Returns the strategy name of the currently selected render lane.
    fn select_lane_name(&self) -> &'static str {
        match self.strategy {
            RenderingStrategy::Unlit => "SimpleUnlit",
            RenderingStrategy::LitForward => "LitForward",
            RenderingStrategy::ForwardPlus => "ForwardPlus",
            RenderingStrategy::Auto => {
                let total_lights = self.render_world.directional_light_count()
                    + self.render_world.point_light_count()
                    + self.render_world.spot_light_count();

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

    /// Selects the appropriate render lane based on the current strategy.
    pub fn select_lane(&self) -> &dyn Lane {
        let name = self.select_lane_name();
        self.lanes.get(name).unwrap_or_else(|| {
            self.lanes
                .find_by_kind(LaneKind::Render)
                .first()
                .copied()
                .expect("RenderAgent has no render lanes configured")
        })
    }

    /// Prepares all rendering data for the current frame.
    ///
    /// This method runs the entire Control Plane logic for rendering:
    /// 1. Prepares GPU resources for any newly loaded meshes.
    /// 2. Extracts all visible objects from the ECS into the internal `RenderWorld`.
    pub fn prepare_frame(&mut self, world: &mut World, graphics_device: &dyn GraphicsDevice) {
        log::trace!("RenderAgent: prepare_frame called");

        self.mesh_preparation_system.run(world, graphics_device);

        log::trace!("RenderAgent: Running extract_lane");
        self.extract_lane.run(world, &mut self.render_world);
        log::trace!(
            "RenderAgent: Extracted {} meshes, {} views",
            self.render_world.meshes.len(),
            self.render_world.views.len()
        );
    }

    /// Translates the prepared data from the `RenderWorld` into a list of `RenderObject`s.
    ///
    /// This method should be called after `prepare_frame`. It reads the intermediate
    /// `RenderWorld` and produces the final, low-level data structure required by
    /// the `RenderSystem`.
    ///
    /// Uses the selected lane's domain-specific pipeline selection if available,
    /// otherwise falls back to a default pipeline.
    pub fn produce_render_objects(&self) -> Vec<RenderObject> {
        let mut render_objects = Vec::with_capacity(self.render_world.meshes.len());
        let gpu_meshes_guard = self.gpu_meshes.read().unwrap();

        // Downcast to concrete lane types to get pipeline selection.
        let selected = self.select_lane();
        let get_pipeline = |material: Option<
            &khora_core::asset::AssetHandle<Box<dyn khora_core::asset::Material>>,
        >|
         -> RenderPipelineId {
            if let Some(lane) = selected.as_any().downcast_ref::<SimpleUnlitLane>() {
                return lane.get_pipeline_for_material(material);
            }
            if let Some(lane) = selected.as_any().downcast_ref::<LitForwardLane>() {
                return lane.get_pipeline_for_material(material);
            }
            if let Some(lane) = selected.as_any().downcast_ref::<ForwardPlusLane>() {
                return lane.get_pipeline_for_material(material);
            }
            RenderPipelineId(0)
        };

        for extracted_mesh in &self.render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_meshes_guard.get(&extracted_mesh.cpu_mesh_uuid) {
                let pipeline = get_pipeline(extracted_mesh.material.as_ref());

                render_objects.push(RenderObject {
                    pipeline,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_count: gpu_mesh_handle.index_count,
                });
            }
        }

        render_objects
    }

    /// Extracts the active camera from the ECS world and generates a `ViewInfo`.
    ///
    /// This method queries the ECS for entities with both a `Camera` and `GlobalTransform`
    /// component, finds the first active camera, and constructs a ViewInfo containing
    /// the camera's view and projection matrices.
    ///
    /// # Arguments
    ///
    /// * `world`: The ECS world containing camera entities
    ///
    /// # Returns
    ///
    /// A `ViewInfo` containing the camera's matrices and position. If no active camera
    /// is found, returns a default ViewInfo with identity matrices.
    pub fn extract_camera_view(&self, world: &World) -> ViewInfo {
        // Query for entities with Camera and GlobalTransform components
        let query = world.query::<(&Camera, &GlobalTransform)>();
        let cameras: Vec<_> = query.collect();
        log::trace!("Found {} cameras in scene", cameras.len());

        // Find the first active camera
        for (camera, global_transform) in cameras {
            log::trace!("Checking camera: is_active={}", camera.is_active);
            if camera.is_active {
                // Extract camera position from the global transform
                let camera_position = global_transform.0.translation();

                // Calculate the view matrix from the global transform
                // The view matrix is the inverse of the camera's world transform
                let view_matrix = if let Some(inv) = global_transform.to_matrix().inverse() {
                    inv
                } else {
                    log::warn!("Failed to invert camera transform, using identity");
                    Mat4::IDENTITY
                };

                // Get the projection matrix from the camera
                let projection_matrix = camera.projection_matrix();

                log::trace!("Camera extracted at position: {:?}", camera_position);
                return ViewInfo::new(view_matrix, projection_matrix, camera_position);
            }
        }

        // No active camera found, return default
        log::warn!("No active camera found in scene, using default ViewInfo");
        ViewInfo::default()
    }

    /// Emits a `GpuReport` telemetry event to the DCC with current frame metrics.
    fn emit_telemetry(&self) {
        if let Some(sender) = &self.telemetry_sender {
            let report = GpuReport {
                frame_number: self.frame_count,
                draw_calls: self.draw_call_count,
                triangles_rendered: self.triangle_count,
                ..Default::default()
            };
            let _ = sender.send(TelemetryEvent::GpuReport(report));
        }
    }

    /// Counts the total triangles in the current render world.
    fn count_triangles(&self) -> u32 {
        let gpu_meshes_guard = match self.gpu_meshes.read() {
            Ok(guard) => guard,
            Err(_) => return 0,
        };
        let mut total = 0u32;
        for mesh in &self.render_world.meshes {
            if let Some(gpu_mesh) = gpu_meshes_guard.get(&mesh.cpu_mesh_uuid) {
                total += match gpu_mesh.primitive_topology {
                    PrimitiveTopology::TriangleList => gpu_mesh.index_count / 3,
                    PrimitiveTopology::TriangleStrip => gpu_mesh.index_count.saturating_sub(2),
                    _ => 0,
                };
            }
        }
        total
    }

    /// Returns the duration of the last frame's render pass.
    pub fn last_frame_time(&self) -> Duration {
        self.last_frame_time
    }

    /// Returns the total number of frames rendered.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Returns the current GORNA strategy ID.
    pub fn current_strategy_id(&self) -> StrategyId {
        self.current_strategy
    }

    /// Returns a reference to the internal RenderWorld.
    pub fn render_world(&self) -> &RenderWorld {
        &self.render_world
    }

    /// Returns a mutable reference to the internal RenderWorld.
    pub fn render_world_mut(&mut self) -> &mut RenderWorld {
        &mut self.render_world
    }

    /// Returns a reference to the GPU meshes cache.
    pub fn gpu_meshes(&self) -> &Arc<RwLock<Assets<GpuMesh>>> {
        &self.gpu_meshes
    }
}

impl Default for RenderAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::control::gorna::{NegotiationRequest, ResourceConstraints, StrategyId};
    use khora_core::math::{Mat4, Vec3};
    use khora_core::renderer::light::{LightType, PointLight};
    use khora_lanes::render_lane::ExtractedLight;
    use std::time::Duration;

    fn dummy_light(light_type: LightType) -> ExtractedLight {
        ExtractedLight {
            light_type,
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        }
    }

    #[test]
    fn test_strategy_selection_auto() {
        let mut agent = RenderAgent::with_strategy(RenderingStrategy::Auto);

        // 0 lights -> SimpleUnlit
        assert_eq!(agent.select_lane_name(), "SimpleUnlit");

        // 1 light -> LitForward
        agent
            .render_world_mut()
            .lights
            .push(dummy_light(LightType::Point(PointLight::default())));
        assert_eq!(agent.select_lane_name(), "LitForward");

        // 21 lights -> ForwardPlus
        for _ in 0..20 {
            agent
                .render_world_mut()
                .lights
                .push(dummy_light(LightType::Point(PointLight::default())));
        }
        assert_eq!(agent.select_lane_name(), "ForwardPlus");
    }

    #[test]
    fn test_negotiation_vram_limits() {
        let mut agent = RenderAgent::new();

        // 1. Unconstrained: should return LowPower (Unlit), Balanced (LitForward), HighPerformance (ForwardPlus)
        let req_unconstrained = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: ResourceConstraints::default(),
        };
        let res = agent.negotiate(req_unconstrained);
        assert_eq!(res.strategies.len(), 3);

        // 2. Tightly constrained VRAM (10 bytes max is too small for Balanced/HighPerformance)
        let req_constrained = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: ResourceConstraints {
                max_vram_bytes: Some(10),
                ..Default::default()
            },
        };
        let res2 = agent.negotiate(req_constrained);
        assert_eq!(res2.strategies.len(), 1);
        assert_eq!(res2.strategies[0].id, StrategyId::LowPower);
    }

    #[test]
    fn test_report_status_health() {
        let mut agent = RenderAgent::new();
        // initially frame count is 0, should be healthy
        let status = agent.report_status();
        assert_eq!(status.health_score, 1.0);

        // Frame count > 0 and budget 10ms, but took 20ms
        agent.frame_count = 1;
        agent.time_budget = Duration::from_millis(10);
        agent.last_frame_time = Duration::from_millis(20);
        let status = agent.report_status();
        assert_eq!(status.health_score, 0.5); // 10 / 20

        // At budget
        agent.last_frame_time = Duration::from_millis(10);
        let status = agent.report_status();
        assert_eq!(status.health_score, 1.0);

        // Under budget
        agent.last_frame_time = Duration::from_millis(5);
        let status = agent.report_status();
        assert_eq!(status.health_score, 1.0); // min(1.0, 10/5=2.0)
    }
}
