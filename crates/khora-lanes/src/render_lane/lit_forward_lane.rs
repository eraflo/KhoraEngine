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

//! Implements a lit forward rendering strategy with shader complexity tracking.
//!
//! The `LitForwardLane` is a rendering pipeline that performs lighting calculations
//! in the fragment shader using a forward rendering approach. It supports multiple
//! light types and tracks shader complexity for GORNA resource negotiation.
//!
//! # Shader Complexity Tracking
//!
//! The cost estimation for this lane includes a shader complexity factor that scales
//! with the number of lights in the scene. This allows GORNA to make informed decisions
//! about rendering strategy selection based on performance budgets.

use crate::render_lane::RenderLane;

use super::RenderWorld;
use khora_core::{
    asset::{AssetUUID, Material},
    renderer::{
        api::{
            command::{
                LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
            },
            PrimitiveTopology,
        },
        traits::CommandEncoder,
        GpuMesh, RenderContext, RenderPipelineId,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

/// Constants for cost estimation.
const TRIANGLE_COST: f32 = 0.001;
const DRAW_CALL_COST: f32 = 0.1;
/// Cost multiplier per light in the scene.
const LIGHT_COST_FACTOR: f32 = 0.05;

/// Shader complexity levels for resource budgeting and GORNA negotiation.
///
/// This enum represents the relative computational cost of different shader
/// configurations, allowing the rendering system to communicate workload
/// estimates to the resource allocation system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ShaderComplexity {
    /// No lighting calculations, vertex colors only.
    /// Fastest rendering path.
    Unlit,
    /// Basic Lambertian diffuse + simple specular.
    /// Moderate performance cost.
    #[default]
    SimpleLit,
    /// Full PBR with Cook-Torrance BRDF.
    /// Highest quality, highest cost.
    FullPBR,
}

impl ShaderComplexity {
    /// Returns a cost multiplier for the given complexity level.
    ///
    /// This multiplier is applied to the base rendering cost to estimate
    /// the total GPU workload for different shader configurations.
    pub fn cost_multiplier(&self) -> f32 {
        match self {
            ShaderComplexity::Unlit => 1.0,
            ShaderComplexity::SimpleLit => 1.5,
            ShaderComplexity::FullPBR => 2.5,
        }
    }

    /// Returns a human-readable name for this complexity level.
    pub fn name(&self) -> &'static str {
        match self {
            ShaderComplexity::Unlit => "Unlit",
            ShaderComplexity::SimpleLit => "SimpleLit",
            ShaderComplexity::FullPBR => "FullPBR",
        }
    }
}

/// A lane that implements forward rendering with lighting support.
///
/// This lane renders meshes with lighting calculations performed in the fragment
/// shader. It supports multiple light types (directional, point, spot) and
/// includes shader complexity tracking for GORNA resource negotiation.
///
/// # Performance Characteristics
///
/// - **O(meshes × lights)** fragment shader complexity
/// - **Suitable for**: Scenes with moderate light counts (< 20 lights)
/// - **Shader complexity tracking**: Integrates with GORNA for adaptive quality
///
/// # Cost Estimation
///
/// The cost estimation includes:
/// - Base triangle and draw call costs (same as `SimpleUnlitLane`)
/// - Shader complexity multiplier based on the configured complexity level
/// - Per-light cost scaling based on the number of active lights
#[derive(Debug)]
pub struct LitForwardLane {
    /// The shader complexity level to use for cost estimation.
    pub shader_complexity: ShaderComplexity,
    /// Maximum number of directional lights supported per pass.
    pub max_directional_lights: u32,
    /// Maximum number of point lights supported per pass.
    pub max_point_lights: u32,
    /// Maximum number of spot lights supported per pass.
    pub max_spot_lights: u32,
}

impl Default for LitForwardLane {
    fn default() -> Self {
        Self {
            shader_complexity: ShaderComplexity::SimpleLit,
            max_directional_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 8,
        }
    }
}

impl LitForwardLane {
    /// Creates a new `LitForwardLane` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new `LitForwardLane` with the specified shader complexity.
    pub fn with_complexity(complexity: ShaderComplexity) -> Self {
        Self {
            shader_complexity: complexity,
            ..Default::default()
        }
    }

    /// Returns the effective number of lights that will be used for rendering.
    ///
    /// This clamps the actual light counts to the maximum supported per pass.
    pub fn effective_light_counts(&self, render_world: &RenderWorld) -> (usize, usize, usize) {
        let dir_count = render_world
            .directional_light_count()
            .min(self.max_directional_lights as usize);
        let point_count = render_world
            .point_light_count()
            .min(self.max_point_lights as usize);
        let spot_count = render_world
            .spot_light_count()
            .min(self.max_spot_lights as usize);

        (dir_count, point_count, spot_count)
    }

    /// Calculates the light-based cost factor for the current frame.
    fn light_cost_factor(&self, render_world: &RenderWorld) -> f32 {
        let (dir_count, point_count, spot_count) = self.effective_light_counts(render_world);
        let total_lights = dir_count + point_count + spot_count;

        // Base cost of 1.0 even with no lights (ambient only)
        1.0 + (total_lights as f32 * LIGHT_COST_FACTOR)
    }
}

impl RenderLane for LitForwardLane {
    fn strategy_name(&self) -> &'static str {
        "LitForward"
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
                let _ = uuid;
            }
        }

        // Currently all lit materials use the same pipeline (ID 1).
        // Future work: differentiate based on material properties,
        // texture presence, alpha mode, etc.
        RenderPipelineId(1)
    }

    fn render(
        &self,
        render_world: &RenderWorld,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
        materials: &RwLock<Assets<Box<dyn Material>>>,
    ) {
        // Acquire read locks on the caches
        let gpu_mesh_assets = gpu_meshes.read().unwrap();
        let material_assets = materials.read().unwrap();

        // Pre-compute all pipelines for each mesh
        let pipelines: Vec<RenderPipelineId> = render_world
            .meshes
            .iter()
            .map(|mesh| self.get_pipeline_for_material(mesh.material_uuid, &material_assets))
            .collect();

        // Configure the render pass
        let color_attachment = RenderPassColorAttachment {
            view: render_ctx.color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(render_ctx.clear_color),
                store: StoreOp::Store,
            },
        };

        let render_pass_desc = RenderPassDescriptor {
            label: Some("Lit Forward Pass"),
            color_attachments: &[color_attachment],
        };

        // Begin the render pass
        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Track the last pipeline to avoid redundant state changes
        let mut current_pipeline: Option<RenderPipelineId> = None;

        // Note: In a full implementation, we would bind light uniform buffers here.
        // The light data from render_world.lights would be uploaded to GPU and bound.
        // For now, this is a placeholder for the rendering logic.

        // Iterate over all extracted meshes and issue draw calls
        for (i, extracted_mesh) in render_world.meshes.iter().enumerate() {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.gpu_mesh_uuid) {
                let pipeline = &pipelines[i];

                // Only bind pipeline if it changed
                if current_pipeline != Some(*pipeline) {
                    render_pass.set_pipeline(pipeline);
                    current_pipeline = Some(*pipeline);
                }

                // Bind vertex buffer
                render_pass.set_vertex_buffer(0, &gpu_mesh_handle.vertex_buffer, 0);

                // Bind index buffer
                render_pass.set_index_buffer(
                    &gpu_mesh_handle.index_buffer,
                    0,
                    gpu_mesh_handle.index_format,
                );

                // Issue draw call
                render_pass.draw_indexed(0..gpu_mesh_handle.index_count, 0, 0..1);
            }
        }
    }

    fn estimate_cost(
        &self,
        render_world: &RenderWorld,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
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
                    PrimitiveTopology::LineList
                    | PrimitiveTopology::LineStrip
                    | PrimitiveTopology::PointList => 0,
                };

                total_triangles += triangle_count;
                draw_call_count += 1;
            }
        }

        // Base cost from triangles and draw calls
        let base_cost =
            (total_triangles as f32 * TRIANGLE_COST) + (draw_call_count as f32 * DRAW_CALL_COST);

        // Apply shader complexity multiplier
        let shader_factor = self.shader_complexity.cost_multiplier();

        // Apply light-based cost scaling
        let light_factor = self.light_cost_factor(render_world);

        // Total cost combines all factors
        base_cost * shader_factor * light_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_lane::world::ExtractedMesh;
    use khora_core::{
        asset::{AssetHandle, AssetUUID},
        math::affine_transform::AffineTransform,
        renderer::{api::PrimitiveTopology, light::DirectionalLight, BufferId, IndexFormat},
    };
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

        let cost = lane.estimate_cost(&render_world, &gpu_meshes);
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
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
        use crate::render_lane::world::ExtractedLight;
        use khora_core::{math::Vec3, renderer::light::LightType};

        let lane = LitForwardLane::new();

        // Create a GPU mesh
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh = create_test_gpu_mesh(300);

        let mut gpu_meshes = Assets::<GpuMesh>::new();
        gpu_meshes.insert(mesh_uuid, AssetHandle::new(gpu_mesh));

        let mut render_world = RenderWorld::default();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::default(),
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        // Add 4 directional lights
        for _ in 0..4 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Directional(DirectionalLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::new(0.0, -1.0, 0.0),
            });
        }

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));
        let cost = lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
            gpu_mesh_uuid: mesh_uuid,
            material_uuid: None,
        });

        let gpu_meshes_lock = Arc::new(RwLock::new(gpu_meshes));

        let unlit_lane = LitForwardLane::with_complexity(ShaderComplexity::Unlit);
        let simple_lane = LitForwardLane::with_complexity(ShaderComplexity::SimpleLit);
        let pbr_lane = LitForwardLane::with_complexity(ShaderComplexity::FullPBR);

        let unlit_cost = unlit_lane.estimate_cost(&render_world, &gpu_meshes_lock);
        let simple_cost = simple_lane.estimate_cost(&render_world, &gpu_meshes_lock);
        let pbr_cost = pbr_lane.estimate_cost(&render_world, &gpu_meshes_lock);

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
        use crate::render_lane::world::ExtractedLight;
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
            });
        }

        // Add 3 point lights (max is 4)
        for _ in 0..3 {
            render_world.lights.push(ExtractedLight {
                light_type: LightType::Point(PointLight::default()),
                position: Vec3::ZERO,
                direction: Vec3::ZERO,
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
        let materials = Assets::<Box<dyn Material>>::new();

        // No material should return pipeline ID 1 (lit default)
        let pipeline = lane.get_pipeline_for_material(None, &materials);
        assert_eq!(pipeline, RenderPipelineId(1));

        // Non-existent material should also return default
        let pipeline = lane.get_pipeline_for_material(Some(AssetUUID::new()), &materials);
        assert_eq!(pipeline, RenderPipelineId(1));
    }
}
