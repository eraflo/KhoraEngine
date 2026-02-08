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
use khora_core::{
    agent::Agent,
    asset::Material,
    control::gorna::{
        AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyOption,
    },
    math::Mat4,
    renderer::{
        api::{GpuMesh, RenderContext, RenderObject},
        traits::CommandEncoder,
        GraphicsDevice, Mesh, ViewInfo,
    },
    EngineContext,
};
use khora_data::{
    assets::Assets,
    ecs::{Camera, GlobalTransform, World},
};
use khora_lanes::render_lane::{
    ExtractRenderablesLane, ForwardPlusLane, LitForwardLane, RenderLane, RenderWorld,
    SimpleUnlitLane,
};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use khora_core::control::gorna::{AgentId, StrategyId};
use khora_core::renderer::api::PrimitiveTopology;

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
    // Available render lanes (strategies). Not destroyed on strategy change.
    lanes: Vec<Box<dyn RenderLane>>,
    // Current rendering strategy selection mode.
    strategy: RenderingStrategy,
    // Current active strategy ID from negotiation.
    current_strategy: StrategyId,
    // Cached reference to the graphics device for lane lifecycle management.
    device: Option<Arc<dyn GraphicsDevice>>,
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

        // Build strategy options from each available lane using actual cost data.
        for lane in &self.lanes {
            let cost = lane.estimate_cost(&self.render_world, &self.gpu_meshes);
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
        match budget.strategy_id {
            StrategyId::LowPower => {
                self.strategy = RenderingStrategy::Unlit;
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
        // Cache the device for future lifecycle calls
        if self.device.is_none() {
            self.device = Some(context.graphics_device.clone());

            // Initialize existing lanes if they were created without a device
            if let Some(device) = &self.device {
                for lane in &self.lanes {
                    if let Err(e) = lane.on_initialize(device.as_ref()) {
                        log::error!(
                            "Failed to initialize render lane {}: {}",
                            lane.strategy_name(),
                            e
                        );
                    }
                }
            }
        }

        // Step 1: Downcast the World and Asset Registry from the type-erased context.
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                // Step 2: Access the CPU mesh assets.
                if let Some(assets_any) = context.assets {
                    if let Some(mesh_assets) = assets_any.downcast_ref::<Assets<Mesh>>() {
                        // Step 3: Run the preparation and extraction logic.
                        self.prepare_frame(world, mesh_assets, context.graphics_device.as_ref());
                        log::trace!("RenderAgent: Tactical update complete. Frame data prepared.");
                    }
                }
            }
        }
    }

    fn report_status(&self) -> AgentStatus {
        let health_score = if self.time_budget.is_zero() || self.frame_count == 0 {
            // No budget assigned yet or no frames rendered — report healthy.
            1.0
        } else {
            // Health = how well we fit within the GORNA time budget.
            // 1.0 = at or under budget, <1.0 = over budget.
            let ratio = self.time_budget.as_secs_f32()
                / self.last_frame_time.as_secs_f32().max(0.0001);
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
}

impl RenderAgent {
    /// Creates a new `RenderAgent` with default lanes and automatic strategy selection.
    pub fn new() -> Self {
        let gpu_meshes = Arc::new(RwLock::new(Assets::new()));

        // Default set of available lanes
        let lanes: Vec<Box<dyn RenderLane>> = vec![
            Box::new(SimpleUnlitLane::new()),
            Box::new(LitForwardLane::new()),
            Box::new(ForwardPlusLane::new()),
        ];

        Self {
            render_world: RenderWorld::new(),
            gpu_meshes: gpu_meshes.clone(),
            mesh_preparation_system: MeshPreparationSystem::new(gpu_meshes),
            extract_lane: ExtractRenderablesLane::new(),
            lanes,
            strategy: RenderingStrategy::Auto,
            current_strategy: StrategyId::Balanced,
            device: None,
            last_frame_time: Duration::ZERO,
            time_budget: Duration::ZERO,
            draw_call_count: 0,
            triangle_count: 0,
            frame_count: 0,
        }
    }

    /// Creates a new `RenderAgent` with the specified rendering strategy.
    pub fn with_strategy(strategy: RenderingStrategy) -> Self {
        let mut agent = Self::new();
        agent.strategy = strategy;
        agent
    }

    /// Adds a custom render lane to the available lanes.
    pub fn add_lane(&mut self, lane: Box<dyn RenderLane>) {
        self.lanes.push(lane);
    }

    /// Sets the rendering strategy.
    pub fn set_strategy(&mut self, strategy: RenderingStrategy) {
        self.strategy = strategy;
    }

    /// Returns the current rendering strategy.
    pub fn strategy(&self) -> RenderingStrategy {
        self.strategy
    }

    /// Returns a reference to the available lanes.
    pub fn lanes(&self) -> &[Box<dyn RenderLane>] {
        &self.lanes
    }

    /// Finds a lane by its strategy name.
    fn find_lane_by_name(&self, name: &str) -> Option<&dyn RenderLane> {
        self.lanes
            .iter()
            .find(|lane| lane.strategy_name() == name)
            .map(|boxed| boxed.as_ref())
    }

    /// Returns the first available lane.
    ///
    /// # Panics
    ///
    /// Panics if no lanes are configured (should never happen after `new()`).
    fn first_lane(&self) -> &dyn RenderLane {
        self.lanes
            .first()
            .map(|b| b.as_ref())
            .expect("RenderAgent has no lanes configured")
    }

    /// Selects the appropriate render lane based on the current strategy.
    pub fn select_lane(&self) -> &dyn RenderLane {
        match self.strategy {
            RenderingStrategy::Unlit => self
                .find_lane_by_name("SimpleUnlit")
                .unwrap_or_else(|| self.first_lane()),
            RenderingStrategy::LitForward => self
                .find_lane_by_name("LitForward")
                .unwrap_or_else(|| self.first_lane()),
            RenderingStrategy::ForwardPlus => self
                .find_lane_by_name("ForwardPlus")
                .unwrap_or_else(|| self.first_lane()),
            RenderingStrategy::Auto => {
                let total_lights = self.render_world.directional_light_count()
                    + self.render_world.point_light_count()
                    + self.render_world.spot_light_count();

                if total_lights > FORWARD_PLUS_LIGHT_THRESHOLD {
                    self.find_lane_by_name("ForwardPlus")
                        .unwrap_or_else(|| self.first_lane())
                } else if total_lights > 0 {
                    self.find_lane_by_name("LitForward")
                        .unwrap_or_else(|| self.first_lane())
                } else {
                    self.find_lane_by_name("SimpleUnlit")
                        .unwrap_or_else(|| self.first_lane())
                }
            }
        }
    }

    /// Prepares all rendering data for the current frame.
    ///
    /// This method runs the entire Control Plane logic for rendering:
    /// 1. Prepares GPU resources for any newly loaded meshes.
    /// 2. Extracts all visible objects from the ECS into the internal `RenderWorld`.
    pub fn prepare_frame(
        &mut self,
        world: &mut World,
        cpu_meshes: &Assets<Mesh>,
        graphics_device: &dyn GraphicsDevice,
    ) {
        // First, ensure all necessary GpuMeshes are created and cached.
        self.mesh_preparation_system
            .run(world, cpu_meshes, graphics_device);

        // Then, extract the prepared renderable data into our local RenderWorld.
        self.extract_lane.run(world, &mut self.render_world);
    }

    /// Renders a frame by preparing the scene data and encoding GPU commands.
    ///
    /// This is the main rendering method that orchestrates the entire rendering pipeline:
    /// 1. Calls `prepare_frame()` to extract and prepare all renderable data
    /// 2. Calls `produce_render_objects()` to build the RenderObject list with proper pipelines
    /// 3. Delegates to the selected render lane to encode GPU commands
    ///
    /// # Arguments
    ///
    /// * `world`: The ECS world containing scene data
    /// * `cpu_meshes`: The cache of CPU-side mesh assets
    /// * `graphics_device`: The graphics device for GPU resource creation
    /// * `materials`: The cache of material assets
    /// * `encoder`: The command encoder to record GPU commands into
    /// * `color_target`: The texture view to render into (typically the swapchain)
    /// * `clear_color`: The color to clear the framebuffer with
    pub fn render(
        &mut self,
        world: &mut World,
        cpu_meshes: &Assets<Mesh>,
        graphics_device: &dyn GraphicsDevice,
        materials: &RwLock<Assets<Box<dyn Material>>>,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
    ) {
        let frame_start = Instant::now();

        // Step 1: Prepare the frame (extract and prepare data).
        self.prepare_frame(world, cpu_meshes, graphics_device);

        // Step 2: Update per-frame metrics from the extracted scene.
        self.draw_call_count = self.render_world.meshes.len() as u32;
        self.triangle_count = self.count_triangles();

        // Step 3: Delegate to the selected render lane to encode GPU commands.
        self.select_lane().render(
            &self.render_world,
            graphics_device,
            encoder,
            render_ctx,
            &self.gpu_meshes,
            materials,
        );

        // Step 4: Record frame timing for GORNA health reporting.
        self.last_frame_time = frame_start.elapsed();
        self.frame_count += 1;
    }

    /// Translates the prepared data from the `RenderWorld` into a list of `RenderObject`s.
    ///
    /// This method should be called after `prepare_frame`. It reads the intermediate
    /// `RenderWorld` and produces the final, low-level data structure required by
    /// the `RenderSystem`.
    ///
    /// This logic uses the render lane to determine the appropriate pipeline for each
    /// material, then builds the RenderObjects list.
    ///
    /// # Arguments
    ///
    /// * `materials`: The cache of material assets for pipeline selection
    pub fn produce_render_objects(
        &self,
        materials: &RwLock<Assets<Box<dyn Material>>>,
    ) -> Vec<RenderObject> {
        let mut render_objects = Vec::with_capacity(self.render_world.meshes.len());
        let gpu_meshes_guard = self.gpu_meshes.read().unwrap();
        let materials_guard = materials.read().unwrap();

        for extracted_mesh in &self.render_world.meshes {
            // Find the corresponding GpuMesh in the cache
            if let Some(gpu_mesh_handle) = gpu_meshes_guard.get(&extracted_mesh.gpu_mesh_uuid) {
                // Use the selected render lane to determine the appropriate pipeline
                let lane = self.select_lane();
                let pipeline =
                    lane.get_pipeline_for_material(extracted_mesh.material_uuid, &materials_guard);

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

        // Find the first active camera
        for (camera, global_transform) in query {
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

                return ViewInfo::new(view_matrix, projection_matrix, camera_position);
            }
        }

        // No active camera found, return default
        log::warn!("No active camera found in scene, using default ViewInfo");
        ViewInfo::default()
    }

    /// Counts the total triangles in the current render world.
    fn count_triangles(&self) -> u32 {
        let gpu_meshes_guard = match self.gpu_meshes.read() {
            Ok(guard) => guard,
            Err(_) => return 0,
        };
        let mut total = 0u32;
        for mesh in &self.render_world.meshes {
            if let Some(gpu_mesh) = gpu_meshes_guard.get(&mesh.gpu_mesh_uuid) {
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
}

impl Default for RenderAgent {
    fn default() -> Self {
        Self::new()
    }
}
