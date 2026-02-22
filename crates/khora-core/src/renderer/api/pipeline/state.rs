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

use super::enums::*;
use crate::khora_bitflags;
use crate::renderer::api::util::enums::{IndexFormat, TextureFormat};
use std::borrow::Cow;

/// Describes a single vertex attribute within a vertex buffer layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VertexAttributeDescriptor {
    /// The input location of this attribute in the vertex shader (e.g., `layout(location = 0)`).
    pub shader_location: u32,
    /// The format of the attribute's data.
    pub format: VertexFormat,
    /// The byte offset of this attribute from the start of the vertex.
    pub offset: u64,
}

/// Describes the memory layout of a single vertex buffer.
#[derive(Debug, Clone)]
pub struct VertexBufferLayoutDescriptor<'a> {
    /// The byte distance between consecutive elements in the buffer.
    pub array_stride: u64,
    /// How often the vertex buffer is advanced.
    pub step_mode: VertexStepMode,
    /// A list of attributes contained within each element of the buffer.
    pub attributes: Cow<'a, [VertexAttributeDescriptor]>,
}

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

/// Describes the stencil test and operations for a single face of a primitive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StencilFaceState {
    /// The comparison function used for the stencil test.
    pub compare: CompareFunction,
    /// The operation to perform if the stencil test fails.
    pub fail_op: StencilOperation,
    /// The operation to perform if the stencil test passes but the depth test fails.
    pub depth_fail_op: StencilOperation,
    /// The operation to perform if both the stencil and depth tests pass.
    pub depth_pass_op: StencilOperation,
}

/// Describes depth biasing, used to prevent z-fighting.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DepthBiasState {
    /// A constant value added to the depth of each fragment.
    pub constant: i32,
    /// A factor that scales with the fragment's depth slope.
    pub slope_scale: f32,
    /// The maximum bias that can be applied.
    pub clamp: f32,
}

/// Describes the state for depth and stencil testing.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthStencilStateDescriptor {
    /// The format of the depth/stencil texture.
    pub format: TextureFormat,
    /// If `true`, depth values will be written to the depth buffer.
    pub depth_write_enabled: bool,
    /// The comparison function used for the depth test.
    pub depth_compare: CompareFunction,
    /// The stencil state for front-facing primitives.
    pub stencil_front: StencilFaceState,
    /// The stencil state for back-facing primitives.
    pub stencil_back: StencilFaceState,
    /// A bitmask for reading from the stencil buffer.
    pub stencil_read_mask: u32,
    /// A bitmask for writing to the stencil buffer.
    pub stencil_write_mask: u32,
    /// The depth bias_state.
    pub bias: DepthBiasState,
}

/// Describes a complete blend equation for a single color component (R, G, B, or A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendComponentDescriptor {
    /// The blend factor for the source color (from the fragment shader).
    pub src_factor: BlendFactor,
    /// The blend factor for the destination color (already in the framebuffer).
    pub dst_factor: BlendFactor,
    /// The operation to combine the source and destination factors.
    pub operation: BlendOperation,
}

/// Describes the blend state for a single color target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendStateDescriptor {
    /// The blend equation for the RGB color components.
    pub color: BlendComponentDescriptor,
    /// The blend equation for the Alpha component.
    pub alpha: BlendComponentDescriptor,
}

khora_bitflags! {
    /// A bitmask to enable or disable writes to individual color channels.
    pub struct ColorWrites: u8 {
        /// Enable writes to the Red channel.
        const R = 0b0001;
        /// Enable writes to the Green channel.
        const G = 0b0010;
        /// Enable writes to the Blue channel.
        const B = 0b0100;
        /// Enable writes to the Alpha channel.
        const A = 0b1000;
        /// Enable writes to all channels.
        const ALL = Self::R.bits() | Self::G.bits() | Self::B.bits() | Self::A.bits();
    }
}

/// Describes the state of a single color target in a render pass.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColorTargetStateDescriptor {
    /// The texture format of this color target.
    pub format: TextureFormat,
    /// The blending state for this target. If `None`, blending is disabled.
    pub blend: Option<BlendStateDescriptor>,
    /// A bitmask controlling which color channels are written to.
    pub write_mask: ColorWrites,
}
