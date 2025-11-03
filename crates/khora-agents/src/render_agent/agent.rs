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
    asset::Material,
    math::LinearRgba,
    renderer::{
        api::{GpuMesh, RenderObject, TextureViewId},
        traits::CommandEncoder,
        GraphicsDevice, Mesh,
    },
};
use khora_data::{assets::Assets, ecs::World};
use khora_lanes::render_lane::{ExtractRenderablesLane, RenderLane, RenderWorld, SimpleUnlitLane};
use std::sync::{Arc, RwLock};

/// The agent responsible for managing the state and logic of the rendering pipeline.
///
/// It orchestrates the various systems and lanes involved in preparing and
/// translating scene data from the ECS into a format consumable by the low-level
/// `RenderSystem`.
pub struct RenderAgent {
    // Intermediate data structure populated by the extraction phase.
    render_world: RenderWorld,
    // Cache for GPU-side mesh assets.
    gpu_meshes: Arc<RwLock<Assets<GpuMesh>>>,
    // System that handles uploading CPU meshes to the GPU.
    mesh_preparation_system: MeshPreparationSystem,
    // Lane that extracts data from the ECS into the RenderWorld.
    extract_lane: ExtractRenderablesLane,
    // Chosen render lane (strategy) as an abstraction.
    render_lane: Box<dyn RenderLane>,
}

impl RenderAgent {
    /// Creates a new `RenderAgent`.
    pub fn new() -> Self {
        let gpu_meshes = Arc::new(RwLock::new(Assets::new()));
        Self {
            render_world: RenderWorld::new(),
            gpu_meshes: gpu_meshes.clone(),
            mesh_preparation_system: MeshPreparationSystem::new(gpu_meshes),
            extract_lane: ExtractRenderablesLane::new(),
            render_lane: Box::new(SimpleUnlitLane::new()),
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
    /// * `encoder`: The command encoder to record GPU commands into
    /// * `color_target`: The texture view to render into (typically the swapchain)
    /// * `materials`: The cache of material assets
    /// * `clear_color`: The color to clear the framebuffer with
    pub fn render(
        &mut self,
        world: &mut World,
        cpu_meshes: &Assets<Mesh>,
        graphics_device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        materials: &RwLock<Assets<Box<dyn Material>>>,
        clear_color: LinearRgba,
    ) {
        // Step 1: Prepare the frame (extract and prepare data)
        self.prepare_frame(world, cpu_meshes, graphics_device);

        // Step 2: Build RenderObjects with proper pipelines
        // (This is where the lane determines which pipeline to use for each material)
        let _render_objects = self.produce_render_objects(materials);

        // Step 3: Delegate to the render lane to encode GPU commands
        self.render_lane.render(
            &self.render_world,
            encoder,
            color_target,
            &self.gpu_meshes,
            materials,
            clear_color,
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
    pub fn produce_render_objects(&self, materials: &RwLock<Assets<Box<dyn Material>>>) -> Vec<RenderObject> {
        let mut render_objects = Vec::with_capacity(self.render_world.meshes.len());
        let gpu_meshes_guard = self.gpu_meshes.read().unwrap();
        let materials_guard = materials.read().unwrap();

        for extracted_mesh in &self.render_world.meshes {
            // Find the corresponding GpuMesh in the cache
            if let Some(gpu_mesh_handle) = gpu_meshes_guard.get(&extracted_mesh.gpu_mesh_uuid) {
                // Use the render lane to determine the appropriate pipeline
                let pipeline = self.render_lane.get_pipeline_for_material(
                    extracted_mesh.material_uuid,
                    &materials_guard,
                );

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
}

impl Default for RenderAgent {
    fn default() -> Self {
        Self::new()
    }
}
