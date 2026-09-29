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
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::lane::Lane;
use khora_core::math::affine_transform::AffineTransform;
use khora_core::math::Mat4;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::resource::{BufferId, IndexFormat};
use khora_core::renderer::light::DirectionalLight;
use khora_data::render::{ExtractedLight, ExtractedMesh};
use std::sync::Arc;

fn create_test_gpu_mesh(index_count: u32) -> GpuMesh {
    GpuMesh {
        vertex_buffer: BufferId(0),
        index_buffer: BufferId(1),
        index_count,
        index_format: IndexFormat::Uint32,
        primitive_topology: PrimitiveTopology::TriangleList,
    }
}

#[test]
fn test_lit_forward_lane_creation() {
    let lane = LitForwardLane::new();
    assert_eq!(lane.strategy_name(), "LitForward");
    assert_eq!(lane.shader_complexity, ShaderComplexity::SimpleLit);
}

#[test]
fn test_lit_forward_lane_with_complexity() {
    let lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);
    assert_eq!(lane.shader_complexity, ShaderComplexity::FullPBR);
}

#[test]
fn test_shader_complexity_ordering() {
    assert!(ShaderComplexity::Unlit < ShaderComplexity::SimpleLit);
    assert!(ShaderComplexity::SimpleLit < ShaderComplexity::FullPBR);
}

#[test]
fn test_shader_complexity_cost_multipliers() {
    assert_eq!(ShaderComplexity::Unlit.cost_multiplier(), 1.0);
    assert_eq!(ShaderComplexity::SimpleLit.cost_multiplier(), 1.5);
    assert_eq!(ShaderComplexity::FullPBR.cost_multiplier(), 2.5);
}

#[test]
fn test_cost_estimation_empty_world() {
    let lane = LitForwardLane::new();
    let render_world = RenderWorld::default();
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));

    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes);
    assert_eq!(cost, 0.0, "Empty world should have zero cost");
}

#[test]
fn test_cost_estimation_with_meshes() {
    let lane = LitForwardLane::new();

    // Create a GPU mesh with 300 indices (100 triangles)
    let mesh_uuid = AssetUUID::new();
    let gpu_mesh = create_test_gpu_mesh(300);

    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: AffineTransform::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Base cost without lights: 100 * 0.001 + 1 * 0.1 = 0.2
    // With SimpleLit multiplier (1.5) and no lights (factor 1.0):
    // 0.2 * 1.5 * 1.0 = 0.3
    assert!(
        (cost - 0.3).abs() < 0.0001,
        "Cost should be 0.3 for 100 triangles with SimpleLit complexity, got {}",
        cost
    );
}

#[test]
fn test_cost_estimation_with_lights() {
    use khora_core::{
        math::{Mat4, Vec3},
        renderer::light::LightType,
    };

    let lane = LitForwardLane::new();

    // Create a GPU mesh
    let mesh_uuid = AssetUUID::new();
    let gpu_mesh = create_test_gpu_mesh(300);

    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: AffineTransform::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
        material: None,
        gpu_material: None,
    });

    // Add 4 directional lights
    for _ in 0..4 {
        render_world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
    }

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
    let cost = lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    // Base cost: 0.2
    // Shader multiplier (SimpleLit): 1.5
    // Light factor: 1.0 + (4 * 0.05) = 1.2
    // Total: 0.2 * 1.5 * 1.2 = 0.36
    assert!(
        (cost - 0.36).abs() < 0.0001,
        "Cost should be 0.36 with 4 lights, got {}",
        cost
    );
}

#[test]
fn test_cost_increases_with_complexity() {
    let mesh_uuid = AssetUUID::new();
    let gpu_mesh = create_test_gpu_mesh(300);

    let mut gpu_meshes = Assets::<GpuMesh>::new();
    gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

    let mut render_world = RenderWorld::default();
    render_world.meshes.push(ExtractedMesh {
        transform: AffineTransform::default(),
        cpu_mesh_uuid: mesh_uuid,
        gpu_mesh: AssetHandle::new(create_test_gpu_mesh(300)),
        material: None,
        gpu_material: None,
    });

    let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));

    let unlit_lane = LitForwardLane::with_complexity(ShaderComplexity::Unlit);
    let simple_lane = LitForwardLane::with_complexity(ShaderComplexity::SimpleLit);
    let pbr_lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);

    let unlit_cost = unlit_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);
    let simple_cost = simple_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);
    let pbr_cost = pbr_lane.estimate_render_cost(&render_world, &gpu_meshes_lock);

    assert!(
        unlit_cost < simple_cost,
        "Unlit should be cheaper than SimpleLit"
    );
    assert!(
        simple_cost < pbr_cost,
        "SimpleLit should be cheaper than PBR"
    );
}

#[test]
fn test_effective_light_counts() {
    use khora_core::{
        math::Vec3,
        renderer::light::{LightType, PointLight},
    };

    let lane = LitForwardLane {
        max_directional_lights: 2,
        max_point_lights: 4,
        max_spot_lights: 2,
        ..Default::default()
    };

    let mut render_world = RenderWorld::default();

    // Add 5 directional lights (max is 2)
    for _ in 0..5 {
        render_world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
    }

    // Add 3 point lights (max is 4)
    for _ in 0..3 {
        render_world.lights.push(ExtractedLight {
            light_type: LightType::Point(PointLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::ZERO,
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
    }

    let (dir, point, spot) = lane.effective_light_counts(&render_world);
    assert_eq!(dir, 2, "Should be clamped to max 2 directional lights");
    assert_eq!(point, 3, "Should use all 3 point lights (under max)");
    assert_eq!(spot, 0, "Should have 0 spot lights");
}

#[test]
fn test_get_pipeline_for_material() {
    let lane = LitForwardLane::new();

    // No GPU init → pipeline not yet created → fallback to RenderPipelineId(0)
    let pipeline = lane.get_pipeline_for_material(None);
    assert_eq!(pipeline, RenderPipelineId(0));

    // Same for repeated calls
    let pipeline = lane.get_pipeline_for_material(None);
    assert_eq!(pipeline, RenderPipelineId(0));
}
