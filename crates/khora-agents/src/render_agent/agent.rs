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
use std::time::Duration;

use khora_core::control::gorna::{AgentId, StrategyId};

/// Threshold for switching to Forward+ rendering.
/// When the scene has more than this many lights, Forward+ is preferred.
const FORWARD_PLUS_LIGHT_THRESHOLD: usize = 20;

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
    // Cache for GPU-side mesh assets.
    gpu_meshes: Arc<RwLock<Assets<GpuMesh>>>,
    // System that handles uploading CPU meshes to the GPU.
    mesh_preparation_system: MeshPreparationSystem,
    // Lane that extracts data from the ECS into the RenderWorld.
    extract_lane: ExtractRenderablesLane,
    // Available render lanes (strategies), extensible collection.
    lanes: Vec<Box<dyn RenderLane>>,
    // Current rendering strategy selection mode.
    strategy: RenderingStrategy,
    // Current active strategy ID from negotiation.
    current_strategy: StrategyId,
}

impl Agent for RenderAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let mut strategies = Vec::new();

        // Strategy 1: Unlit (Low Power / Very Low Cost)
        strategies.push(StrategyOption {
            id: StrategyId::LowPower,
            estimated_time: Duration::from_millis(2),
            estimated_vram: 0, // Differential
        });

        // Strategy 2: LitForward (Balanced / Standard)
        strategies.push(StrategyOption {
            id: StrategyId::Balanced,
            estimated_time: Duration::from_millis(8),
            estimated_vram: 1024 * 1024 * 10, // 10MB approx
        });

        // Strategy 3: ForwardPlus (High Performance / High Cost for many lights)
        let total_lights = self.render_world.directional_light_count()
            + self.render_world.point_light_count()
            + self.render_world.spot_light_count();

        if total_lights > 5 {
            strategies.push(StrategyOption {
                id: StrategyId::HighPerformance,
                estimated_time: Duration::from_millis(12),
                estimated_vram: 1024 * 1024 * 20,
            });
        }

        NegotiationResponse { strategies }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!(
            "RenderAgent: Dynamic strategy update to {:?}",
            budget.strategy_id
        );

        // Clear existing lanes and set up new ones based on the chosen strategy.
        // Note: The ExtractRenderablesLane is typically a separate system that populates RenderWorld,
        // not a RenderLane itself. The provided instruction seems to re-purpose it here.
        // Assuming the intent is to dynamically configure the *rendering* lanes.
        self.lanes.clear();
        match budget.strategy_id {
            StrategyId::LowPower => {
                self.lanes.push(Box::new(SimpleUnlitLane::new()));
                self.strategy = RenderingStrategy::Unlit;
            }
            StrategyId::Balanced => {
                self.lanes.push(Box::new(LitForwardLane::new()));
                self.strategy = RenderingStrategy::LitForward;
            }
            StrategyId::HighPerformance => {
                self.lanes.push(Box::new(ForwardPlusLane::new()));
                self.strategy = RenderingStrategy::ForwardPlus;
            }
            StrategyId::Custom(_) => {
                log::warn!(
                    "RenderAgent received unsupported custom strategy. Falling back to Balanced."
                );
                self.lanes.push(Box::new(LitForwardLane::new()));
                self.strategy = RenderingStrategy::LitForward;
            }
        }

        self.current_strategy = budget.strategy_id;
    }

    fn update(&mut self, context: &mut EngineContext<'_>) {
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
        // The health score currently remains optimal if the agent is correctly
        // operating under its assigned strategy.
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!("Lights: {}", self.render_world.point_light_count()),
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

    /// Selects the appropriate render lane based on the current strategy.
    pub fn select_lane(&self) -> &dyn RenderLane {
        match self.strategy {
            RenderingStrategy::Unlit => self
                .find_lane_by_name("SimpleUnlit")
                .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap()),
            RenderingStrategy::LitForward => self
                .find_lane_by_name("LitForward")
                .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap()),
            RenderingStrategy::ForwardPlus => self
                .find_lane_by_name("ForwardPlus")
                .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap()),
            RenderingStrategy::Auto => {
                // Automatic selection based on light count
                let total_lights = self.render_world.directional_light_count()
                    + self.render_world.point_light_count()
                    + self.render_world.spot_light_count();

                if total_lights > FORWARD_PLUS_LIGHT_THRESHOLD {
                    self.find_lane_by_name("ForwardPlus")
                        .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap())
                } else if total_lights > 0 {
                    self.find_lane_by_name("LitForward")
                        .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap())
                } else {
                    self.find_lane_by_name("SimpleUnlit")
                        .unwrap_or(self.lanes.first().map(|b| b.as_ref()).unwrap())
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
        // Step 1: Prepare the frame (extract and prepare data)
        self.prepare_frame(world, cpu_meshes, graphics_device);

        // Step 2: Build RenderObjects with proper pipelines
        // (This is where the lane determines which pipeline to use for each material)
        let _render_objects = self.produce_render_objects(materials);

        // Step 3: Delegate to the selected render lane to encode GPU commands
        self.select_lane().render(
            &self.render_world,
            encoder,
            render_ctx,
            &self.gpu_meshes,
            materials,
        );
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
                    eprintln!("Warning: Failed to invert camera transform, using identity");
                    Mat4::IDENTITY
                };

                // Get the projection matrix from the camera
                let projection_matrix = camera.projection_matrix();

                return ViewInfo::new(view_matrix, projection_matrix, camera_position);
            }
        }

        // No active camera found, return default
        eprintln!("Warning: No active camera found in scene, using default ViewInfo");
        ViewInfo::default()
    }
}

impl Default for RenderAgent {
    fn default() -> Self {
        Self::new()
    }
}
