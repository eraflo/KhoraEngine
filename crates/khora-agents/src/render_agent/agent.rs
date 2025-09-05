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
use khora_core::renderer::{
    api::{GpuMesh, RenderObject},
    GraphicsDevice, Mesh,
};
use khora_data::{assets::Assets, ecs::World};
use khora_lanes::render_lane::{ExtractRenderablesLane, RenderWorld};
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

    /// Translates the prepared data from the `RenderWorld` into a list of `RenderObject`s.
    ///
    /// This method should be called after `prepare_frame`. It reads the intermediate
    /// `RenderWorld` and produces the final, low-level data structure required by
    /// the `RenderSystem`.
    pub fn produce_render_objects(&self) -> Vec<RenderObject> {
        let mut render_objects = Vec::new();
        let gpu_meshes = self.gpu_meshes.read().unwrap();

        for extracted_mesh in &self.render_world.meshes {
            // Find the corresponding GpuMesh in the cache.
            if let Some(gpu_mesh_handle) = gpu_meshes.get(&extracted_mesh.gpu_mesh_uuid) {
                // The RenderObject requires a pipeline. For now, we don't have a material
                // system, so we can't determine the correct pipeline yet. This is a placeholder.
                // In a real scenario, this would come from the material.
                // TODO: Replace with a real pipeline from a material system.
                let placeholder_pipeline = khora_core::renderer::RenderPipelineId(0);

                render_objects.push(RenderObject {
                    pipeline: placeholder_pipeline,
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
