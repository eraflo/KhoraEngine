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

//! State descriptors for the pipeline.

pub use super::color_target_state_descriptor::{
    BlendComponentDescriptor, BlendStateDescriptor, ColorTargetStateDescriptor, ColorWrites,
};
pub use super::depth_stencil_state_descriptor::{
    DepthBiasState, DepthStencilStateDescriptor, StencilFaceState,
};
use super::enums::*;
pub use super::vertex_buffer_layout_descriptor::{
    VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
};
use crate::renderer::api::resource::IndexFormat;

/// Describes the state for primitive assembly and rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrimitiveStateDescriptor {
    /// The topology of the primitives.
    pub topology: PrimitiveTopology,
    /// The index format to use for `TriangleStrip` and `LineStrip` topologies.
    pub strip_index_format: Option<IndexFormat>,
    /// The vertex winding order that determines the "front" face of a triangle.
    pub front_face: FrontFace,
    /// The face culling mode.
    pub cull_mode: Option<CullMode>,
    /// The rasterization mode for polygons.
    pub polygon_mode: PolygonMode,
    /// If `true`, disables clipping of fragments based on their depth.
    pub unclipped_depth: bool,
    /// If `true`, enables conservative rasterization.
    pub conservative: bool,
}

impl Default for PrimitiveStateDescriptor {
    fn default() -> Self {
        PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        }
    }
}
