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

//! Implements a simple, unlit rendering strategy.
//!
//! The `SimpleUnlitLane` is the most basic rendering pipeline in Khora. It renders
//! meshes without any lighting calculations, making it the fastest and most straightforward
//! rendering strategy. This lane is ideal for:
//! - Debug visualization and prototyping
//! - Rendering UI elements or 2D sprites
//! - Performance-critical scenarios where lighting is not needed
//! - Serving as a fallback when more complex rendering strategies cannot meet their budget
//!
//! As a "Lane" in the CLAD architecture, this implementation is optimized for raw speed
//! and deterministic execution. It contains minimal branching logic and is designed to
//! be driven by a higher-level `RenderAgent`.

use crate::render_lane::RenderLane;

use super::RenderWorld;
use khora_core::{
    asset::{AssetUUID, Material},
    math::LinearRgba,
    renderer::{
        api::{
            command::{LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp},
            PrimitiveTopology,
        },
        traits::CommandEncoder,
        GpuMesh, RenderPipelineId, TextureViewId,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

/// A lane that implements a simple, unlit forward rendering strategy.
///
/// This lane takes the extracted scene data from a `RenderWorld` and generates
/// GPU commands to render all meshes with a basic, unlit appearance. It does not
/// perform any lighting calculations, shadow mapping, or post-processing effects.
///
/// # Performance Characteristics
/// - **Zero heap allocations** during the render pass encoding
/// - **Linear iteration** over the extracted mesh list
/// - **Minimal state changes** (one pipeline bind per material, ideally)
/// - **Suitable for**: High frame rates, simple scenes, or as a debug/fallback renderer
#[derive(Default)]
pub struct SimpleUnlitLane;

impl SimpleUnlitLane {
    /// Creates a new `SimpleUnlitLane`.
    ///
    /// This lane is stateless, so construction is trivial.
    pub fn new() -> Self {
        Self
    }
}

impl RenderLane for SimpleUnlitLane {
    fn strategy_name(&self) -> &'static str {
        "SimpleUnlit"
    }

    fn get_pipeline_for_material(
        &self,
        material_uuid: Option<AssetUUID>,
        materials: &Assets<Box<dyn Material>>,
    ) -> RenderPipelineId {
        // If a material is specified, verify it exists in the cache
        if let Some(uuid) = material_uuid {
            if materials.get(&uuid).is_none() {
                // Material not found, will use default pipeline
                let _ = uuid; // Silence unused warning
            }
        }
        
        // All unlit materials currently use the same pipeline
        // Future enhancements could differentiate based on:
        // - Texture presence (textured vs. untextured)
        // - Alpha blend mode (opaque, masked, transparent)
        // - Two-sided rendering
        RenderPipelineId(0)
    }

    fn render(
        &self,
        render_world: &RenderWorld,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
        materials: &RwLock<Assets<Box<dyn Material>>>,
        clear_color: LinearRgba,
    ) {
        // Acquire read locks on the caches
        let gpu_mesh_assets = gpu_meshes.read().unwrap();
        let material_assets = materials.read().unwrap();

        // Pre-compute all pipelines for each mesh to ensure they live long enough
        // for the render pass references
        let pipelines: Vec<RenderPipelineId> = render_world
            .meshes
            .iter()
            .map(|mesh| self.get_pipeline_for_material(mesh.material_uuid, &material_assets))
            .collect();

        // Configure the render pass to render into the provided color target
        let color_attachment = RenderPassColorAttachment {
            view: color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(clear_color),
                store: StoreOp::Store,
            },
        };

        let render_pass_desc = RenderPassDescriptor {
            label: Some("Simple Unlit Pass"),
            color_attachments: &[color_attachment],
        };

        // Begin the render pass
        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Track the last pipeline we bound to avoid redundant state changes
        let mut current_pipeline: Option<RenderPipelineId> = None;

        // Iterate over all extracted meshes and issue draw calls
        for (i, extracted_mesh) in render_world.meshes.iter().enumerate() {
            // Look up the corresponding GpuMesh in the cache
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.gpu_mesh_uuid) {
                // Get the pre-computed pipeline for this mesh
                let pipeline = &pipelines[i];

                // Only bind the pipeline if it's different from the current one
                // This is a basic optimization to reduce GPU state changes
                if current_pipeline != Some(*pipeline) {
                    render_pass.set_pipeline(pipeline);
                    current_pipeline = Some(*pipeline);
                }

                // Bind the vertex buffer
                render_pass.set_vertex_buffer(0, &gpu_mesh_handle.vertex_buffer, 0);

                // Bind the index buffer with the correct format from the mesh
                render_pass.set_index_buffer(
                    &gpu_mesh_handle.index_buffer,
                    0,
                    gpu_mesh_handle.index_format,
                );

                // Issue the indexed draw call
                render_pass.draw_indexed(0..gpu_mesh_handle.index_count, 0, 0..1);
            }
        }
    }

    fn estimate_cost(&self, render_world: &RenderWorld, gpu_meshes: &RwLock<Assets<GpuMesh>>) -> f32 {
        let gpu_mesh_assets = gpu_meshes.read().unwrap();
        
        let mut total_triangles = 0u32;
        let mut draw_call_count = 0u32;

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh) = gpu_mesh_assets.get(&extracted_mesh.gpu_mesh_uuid) {
                // Calculate triangle count based on primitive topology
                let triangle_count = match gpu_mesh.primitive_topology {
                    PrimitiveTopology::TriangleList => gpu_mesh.index_count / 3,
                    PrimitiveTopology::TriangleStrip => {
                        if gpu_mesh.index_count >= 3 {
                            gpu_mesh.index_count - 2
                        } else {
                            0
                        }
                    }
                    // Lines and points don't contribute to triangle count
                    PrimitiveTopology::LineList
                    | PrimitiveTopology::LineStrip
                    | PrimitiveTopology::PointList => 0,
                };
                
                total_triangles += triangle_count;
                draw_call_count += 1;
            }
        }

        // Cost model: triangles have a small per-triangle cost,
        // draw calls have a fixed overhead
        const TRIANGLE_COST: f32 = 0.001;
        const DRAW_CALL_COST: f32 = 0.1;

        (total_triangles as f32 * TRIANGLE_COST) + (draw_call_count as f32 * DRAW_CALL_COST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::{
        asset::AssetHandle,
        renderer::{api::PrimitiveTopology, BufferId, IndexFormat},
    };
    use std::sync::Arc;

    #[test]
    fn test_simple_unlit_lane_creation() {
        let lane = SimpleUnlitLane::new();
        assert_eq!(lane.strategy_name(), "SimpleUnlit");
    }

    #[test]
    fn test_default_construction() {
        let lane = SimpleUnlitLane::default();
        assert_eq!(lane.strategy_name(), "SimpleUnlit");
    }

    #[test]
    fn test_cost_estimation_empty_world() {
        let lane = SimpleUnlitLane::new();
        let render_world = RenderWorld::default();
        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
        
        let cost = lane.estimate_cost(&render_world, &gpu_meshes);
        assert_eq!(cost, 0.0, "Empty world should have zero cost");
    }

    #[test]
    fn test_cost_estimation_triangle_list() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        
        // Create a GPU mesh with 300 indices (100 triangles) using TriangleList
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 300,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };
        
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));
        
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });
        
        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);
        
        // Expected: 100 triangles * 0.001 + 1 draw call * 0.1 = 0.1 + 0.1 = 0.2
        assert_eq!(cost, 0.2, "Cost should be 0.2 for 100 triangles + 1 draw call");
    }

    #[test]
    fn test_cost_estimation_triangle_strip() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        
        // Create a GPU mesh with 52 indices (50 triangles) using TriangleStrip
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 52,
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };
        
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));
        
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });
        
        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);
        
        // Expected: 50 triangles * 0.001 + 1 draw call * 0.1 = 0.05 + 0.1 = 0.15
        assert_eq!(cost, 0.15, "Cost should be 0.15 for 50 triangles + 1 draw call");
    }

    #[test]
    fn test_cost_estimation_lines_and_points() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        
        // Create meshes with non-triangle topologies
        let line_uuid = AssetUUID::new();
        let point_uuid = AssetUUID::new();
        
        let line_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 100,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::LineList,
        };
        
        let point_mesh = GpuMesh {
            vertex_buffer: BufferId(2),
            index_buffer: BufferId(3),
            index_count: 50,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::PointList,
        };
        
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(line_uuid, AssetHandle::new(line_mesh));
        gpu_meshes.insert(point_uuid, AssetHandle::new(point_mesh));
        
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: line_uuid,
            material_uuid: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: point_uuid,
            material_uuid: None,
        });
        
        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);
        
        // Expected: 0 triangles * 0.001 + 2 draw calls * 0.1 = 0.0 + 0.2 = 0.2
        assert_eq!(cost, 0.2, "Cost should be 0.2 for 2 draw calls with no triangles");
    }

    #[test]
    fn test_cost_estimation_multiple_meshes() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        
        // Create 3 different meshes
        let mesh1_uuid = AssetUUID::new();
        let mesh2_uuid = AssetUUID::new();
        let mesh3_uuid = AssetUUID::new();
        
        let mesh1 = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 600, // 200 triangles
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };
        
        let mesh2 = GpuMesh {
            vertex_buffer: BufferId(2),
            index_buffer: BufferId(3),
            index_count: 102, // 100 triangles (strip)
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };
        
        let mesh3 = GpuMesh {
            vertex_buffer: BufferId(4),
            index_buffer: BufferId(5),
            index_count: 150, // 50 triangles
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        };
        
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh1_uuid, AssetHandle::new(mesh1));
        gpu_meshes.insert(mesh2_uuid, AssetHandle::new(mesh2));
        gpu_meshes.insert(mesh3_uuid, AssetHandle::new(mesh3));
        
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh1_uuid,
            material_uuid: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh2_uuid,
            material_uuid: None,
        });
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh3_uuid,
            material_uuid: None,
        });
        
        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);
        
        // Expected: (200 + 100 + 50) triangles * 0.001 + 3 draw calls * 0.1
        //         = 350 * 0.001 + 3 * 0.1 = 0.35 + 0.3 = 0.65
        assert!((cost - 0.65).abs() < 0.0001, "Cost should be approximately 0.65 for 350 triangles + 3 draw calls, got {}", cost);
    }

    #[test]
    fn test_cost_estimation_missing_mesh() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
        
        // Reference a mesh that doesn't exist in the cache
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: AssetUUID::new(),
            material_uuid: None,
        });
        
        let cost = lane.estimate_cost(&render_world, &gpu_meshes);
        
        // Expected: 0 cost since mesh is not found
        assert_eq!(cost, 0.0, "Missing mesh should contribute zero cost");
    }

    #[test]
    fn test_cost_estimation_degenerate_triangle_strip() {
        use khora_core::asset::AssetUUID;
        use crate::render_lane::world::ExtractedMesh;

        let lane = SimpleUnlitLane::new();
        
        // Create a triangle strip with only 2 indices (not enough for a triangle)
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(1),
            index_count: 2,
            index_format: IndexFormat::Uint16,
            primitive_topology: PrimitiveTopology::TriangleStrip,
        };
        
        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));
        
        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: Default::default(),
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });
        
        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);
        
        // Expected: 0 triangles + 1 draw call * 0.1 = 0.1
        assert_eq!(cost, 0.1, "Degenerate triangle strip should only cost draw call overhead");
    }

    #[test]
    fn test_get_pipeline_for_material_with_none() {
        let lane = SimpleUnlitLane::new();
        let materials = Assets::<Box<dyn Material>>::new();
        
        let pipeline = lane.get_pipeline_for_material(None, &materials);
        assert_eq!(pipeline, RenderPipelineId(0), "None material should use default pipeline");
    }

    #[test]
    fn test_get_pipeline_for_material_not_found() {
        use khora_core::asset::AssetUUID;
        
        let lane = SimpleUnlitLane::new();
        let materials = Assets::<Box<dyn Material>>::new();
        
        let pipeline = lane.get_pipeline_for_material(Some(AssetUUID::new()), &materials);
        assert_eq!(pipeline, RenderPipelineId(0), "Missing material should use default pipeline");
    }
}
