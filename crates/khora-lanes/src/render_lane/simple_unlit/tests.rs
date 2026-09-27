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

use super::*;
use khora_core::lane::Lane;
use khora_core::{
    asset::AssetHandle,
    renderer::api::{pipeline::enums::PrimitiveTopology, resource::BufferId, util::IndexFormat},
};
use khora_data::render::ExtractedMesh;
use std::sync::Arc;

#[test]
fn test_simple_unlit_lane_creation() {
    let lane = SimpleUnlitLane::new();
    assert_eq!(lane.strategy_name(), "SimpleUnlit");
}

#[test]
fn test_default_construction() {
    let lane = SimpleUnlitLane::new();
    assert_eq!(lane.strategy_name(), "SimpleUnlit");
}

#[test]
fn test_cost_estimation_empty_world() {
    let lane = SimpleUnlitLane::new();
    let render_world = RenderWorld::default();
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);
    assert_eq!(cost, 0.0, "Empty world should have zero cost");
}

#[test]
fn test_cost_estimation_triangle_list() {
    use khora_core::asset::AssetUUID;

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
    let gpu_mesh_handle = AssetHandle::new(gpu_mesh);
    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, gpu_mesh_handle.clone());

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: gpu_mesh_handle,
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Expected: 100 triangles * 0.001 + 1 draw call * 0.1 = 0.1 + 0.1 = 0.2
    assert_eq!(
        cost, 0.2,
        "Cost should be 0.2 for 100 triangles + 1 draw call"
    );
}

#[test]
fn test_cost_estimation_triangle_strip() {
    use khora_core::asset::AssetUUID;

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
    let gpu_mesh_handle = AssetHandle::new(gpu_mesh);
    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, gpu_mesh_handle.clone());

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: gpu_mesh_handle,
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Expected: 50 triangles * 0.001 + 1 draw call * 0.1 = 0.05 + 0.1 = 0.15
    assert_eq!(
        cost, 0.15,
        "Cost should be 0.15 for 50 triangles + 1 draw call"
    );
}

#[test]
fn test_cost_estimation_lines_and_points() {
    use khora_core::asset::AssetUUID;

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
    let line_mesh_handle = AssetHandle::new(line_mesh);
    let point_mesh_handle = AssetHandle::new(point_mesh);

    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(line_uuid, line_mesh_handle.clone());
    gpu_meshes.insert(point_uuid, point_mesh_handle.clone());

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: line_uuid,
        gpu_mesh: line_mesh_handle,
        material: None,
        gpu_material: None,
    });
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: point_uuid,
        gpu_mesh: point_mesh_handle,
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Expected: 0 triangles * 0.001 + 2 draw calls * 0.1 = 0.0 + 0.2 = 0.2
    assert_eq!(
        cost, 0.2,
        "Cost should be 0.2 for 2 draw calls with no triangles"
    );
}

#[test]
fn test_cost_estimation_multiple_meshes() {
    use khora_core::asset::AssetUUID;

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
        cpu_mesh_uuid: mesh1_uuid,
        gpu_mesh: AssetHandle::new(create_test_mesh(600)),
        material: None,
        gpu_material: None,
    });
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: mesh2_uuid,
        gpu_mesh: AssetHandle::new(create_test_mesh(102)),
        material: None,
        gpu_material: None,
    });
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: mesh3_uuid,
        gpu_mesh: AssetHandle::new(create_test_mesh(150)),
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Expected: (200 + 100 + 50) triangles * 0.001 + 3 draw calls * 0.1
    //         = 350 * 0.001 + 3 * 0.1 = 0.35 + 0.3 = 0.65
    assert!(
        (cost - 0.65).abs() < 0.0001,
        "Cost should be approximately 0.65 for 350 triangles + 3 draw calls, got {}",
        cost
    );
}

// Helper to create test mesh
fn create_test_mesh(index_count: u32) -> GpuMesh {
    GpuMesh {
        vertex_buffer: BufferId(0),
        index_buffer: BufferId(1),
        index_count,
        index_format: IndexFormat::Uint32,
        primitive_topology: PrimitiveTopology::TriangleList,
    }
}

#[test]
fn test_cost_estimation_missing_mesh() {
    use khora_core::asset::AssetUUID;

    let lane = SimpleUnlitLane::new();
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

    // Reference a mesh that doesn't exist in the cache
    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: AssetUUID::new(),
        gpu_mesh: AssetHandle::new(create_test_mesh(300)),
        material: None,
        gpu_material: None,
    });

    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);

    // Expected: 0 cost since mesh is not found
    assert_eq!(cost, 0.0, "Missing mesh should contribute zero cost");
}

#[test]
fn test_cost_estimation_degenerate_triangle_strip() {
    use khora_core::asset::AssetUUID;

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

    let handle = AssetHandle::new(gpu_mesh);
    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, handle.clone());

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: Default::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: handle,
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Expected: 0 triangles + 1 draw call * 0.1 = 0.1
    assert_eq!(
        cost, 0.1,
        "Degenerate triangle strip should only cost draw call overhead"
    );
}

#[test]
fn test_get_pipeline_for_material_with_none() {
    let lane = SimpleUnlitLane::new();

    let pipeline = lane.get_pipeline_for_material(None);
    assert_eq!(
        pipeline,
        RenderPipelineId(0),
        "None material should use default pipeline"
    );
}

#[test]
fn test_get_pipeline_for_material_not_found() {
    let lane = SimpleUnlitLane::new();

    // Since there is no registry anymore, we just test with None or a dummy handle.
    // The old test "missing material" is now redundant with "None material".
    let pipeline = lane.get_pipeline_for_material(None);
    assert_eq!(
        pipeline,
        RenderPipelineId(0),
        "Missing material should use default pipeline"
    );
}
