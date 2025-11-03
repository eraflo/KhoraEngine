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

//! Rendering lane - hot path for graphics operations

use khora_core::{
    asset::{AssetUUID, Material},
    math::LinearRgba,
    renderer::{
        api::{GpuMesh, TextureViewId},
        traits::CommandEncoder,
        RenderPipelineId,
    },
};
use khora_data::assets::Assets;
use std::sync::RwLock;

mod extract_lane;
mod simple_unlit_lane;
mod world;

pub use extract_lane::*;
pub use simple_unlit_lane::*;
pub use world::*;

/// A trait defining the behavior of a rendering lane.
///
/// Rendering lanes are responsible for encoding GPU commands to render the scene
/// represented in the `RenderWorld`. Different implementations provide different
/// rendering strategies (e.g., forward rendering, deferred rendering, unlit rendering).
///
/// This trait enables the `RenderAgent` to work with multiple rendering strategies
/// without being coupled to any specific implementation, following the CLAD
/// architecture's separation of concerns.
///
/// The lane uses abstractions from `khora-core` (CommandEncoder, TextureViewId, etc.)
/// and does not depend on infrastructure-specific types.
pub trait RenderLane: Send + Sync {
    /// Returns a human-readable identifier for this rendering strategy.
    ///
    /// This can be used by the `RenderAgent` for logging, debugging, and
    /// strategy selection purposes.
    ///
    /// # Returns
    ///
    /// A static string identifying the rendering strategy (e.g., "SimpleUnlit", "ForwardPlus").
    fn strategy_name(&self) -> &'static str;

    /// Gets the appropriate render pipeline for a given material.
    ///
    /// This method determines which pipeline to use based on the material's properties.
    /// Different rendering strategies may select different pipelines based on material
    /// characteristics (e.g., alpha blending, two-sided rendering, texturing).
    ///
    /// # Arguments
    ///
    /// * `material_uuid`: The UUID of the material, if any
    /// * `materials`: The cache of Material assets
    ///
    /// # Returns
    ///
    /// The `RenderPipelineId` to use for rendering with this material.
    fn get_pipeline_for_material(
        &self,
        material_uuid: Option<AssetUUID>,
        materials: &Assets<Box<dyn Material>>,
    ) -> RenderPipelineId;

    /// Encodes GPU commands to render the scene into the provided command encoder.
    ///
    /// This is the main rendering method that translates the `RenderWorld` into
    /// actual GPU drawing commands using the abstractions from `khora-core`.
    ///
    /// # Arguments
    ///
    /// * `render_world`: The intermediate world containing extracted mesh data.
    /// * `encoder`: The command encoder to record GPU commands into (from core traits).
    /// * `color_target`: The texture view to render into.
    /// * `gpu_meshes`: The cache of GPU-resident meshes.
    /// * `materials`: The cache of materials for pipeline selection.
    /// * `clear_color`: The color to clear the render target with.
    fn render(
        &self,
        render_world: &RenderWorld,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
        materials: &RwLock<Assets<Box<dyn Material>>>,
        clear_color: LinearRgba,
    );

    /// Estimates the GPU cost of rendering the given `RenderWorld` with this strategy.
    ///
    /// This method is used by the `RenderAgent` to negotiate with GORNA (Goal-Oriented
    /// Resource Negotiation & Allocation) and determine which rendering strategy to use
    /// based on available budget and scene complexity.
    ///
    /// The cost is measured in abstract units representing GPU workload (e.g., number
    /// of triangles, draw calls, shader complexity).
    ///
    /// # Arguments
    ///
    /// * `render_world`: The world to estimate the cost for.
    /// * `gpu_meshes`: The GPU mesh cache to look up mesh complexity.
    ///
    /// # Returns
    ///
    /// An estimated cost value. Higher values indicate more expensive rendering.
    fn estimate_cost(&self, render_world: &RenderWorld, gpu_meshes: &RwLock<Assets<GpuMesh>>) -> f32;
}
