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

//! Defines the lane responsible for extracting renderable data from the main ECS world.

use super::{ExtractedMesh, RenderWorld};
use khora_core::renderer::GpuMesh;
use khora_data::ecs::{GlobalTransform, HandleComponent, World};

/// A lane that performs the "extraction" phase of the rendering pipeline.
///
/// It queries the main `World` for entities with renderable components and populates
/// the `RenderWorld` with a simplified, flat representation of the scene suitable
/// for rendering.
#[derive(Default)]
pub struct ExtractRenderablesLane;

impl ExtractRenderablesLane {
    /// Creates a new `ExtractRenderablesLane`.
    pub fn new() -> Self {
        Self
    }

    /// Executes the extraction process for one frame.
    ///
    /// # Arguments
    /// * `world`: A reference to the main ECS `World` containing simulation data.
    /// * `render_world`: A mutable reference to the `RenderWorld` to be populated.
    pub fn run(&self, world: &World, render_world: &mut RenderWorld) {
        // 1. Clear the render world from the previous frame's data.
        render_world.clear();

        // 2. Execute the transversal query to find all renderable meshes.
        let query = world.query::<(&GlobalTransform, &HandleComponent<GpuMesh>)>();

        // 3. Iterate directly over the query and populate the RenderWorld.
        for (transform, gpu_mesh_handle_comp) in query {
            let extracted_mesh = ExtractedMesh {
                // We assume `GlobalTransform` has a method to convert it to a matrix.
                transform: transform.to_matrix(),
                // Clone the AssetHandle from within the component. This is a cheap Arc clone.
                gpu_mesh_handle: gpu_mesh_handle_comp.handle.clone(),
            };
            render_world.meshes.push(extracted_mesh);
        }
    }
}
