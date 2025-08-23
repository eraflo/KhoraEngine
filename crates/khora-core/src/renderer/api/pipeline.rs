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

//! Defines all data structures used to configure a graphics render pipeline.

use crate::{
    khora_bitflags,
    renderer::{IndexFormat, ShaderModuleId, TextureFormat},
};
use std::borrow::Cow;

use super::common::SampleCount;

/// The memory format of a single vertex attribute's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    /// Two 8-bit unsigned integer components.
    Uint8x2,
    /// Four 8-bit unsigned integer components.
    Uint8x4,
    /// Two 8-bit signed integer components.
    Sint8x2,
    /// Four 8-bit signed integer components.
    Sint8x4,
    /// Two 8-bit unsigned integer components normalized to `[0.0, 1.0]`.
    Unorm8x2,
    /// Four 8-bit unsigned integer components normalized to `[0.0, 1.0]`.
    Unorm8x4,
    /// Two 8-bit signed integer components normalized to `[-1.0, 1.0]`.
    Snorm8x2,
    /// Four 8-bit signed integer components normalized to `[-1.0, 1.0]`.
    Snorm8x4,
    /// Two 16-bit unsigned integer components.
    Uint16x2,
    /// Four 16-bit unsigned integer components.
    Uint16x4,
    /// Two 16-bit signed integer components.
    Sint16x2,
    /// Four 16-bit signed integer components.
    Sint16x4,
    /// Two 16-bit unsigned integer components normalized to `[0.0, 1.0]`.
    Unorm16x2,
    /// Four 16-bit unsigned integer components normalized to `[0.0, 1.0]`.
    Unorm16x4,
    /// Two 16-bit signed integer components normalized to `[-1.0, 1.0]`.
    Snorm16x2,
    /// Four 16-bit signed integer components normalized to `[-1.0, 1.0]`.
    Snorm16x4,
    /// Two 16-bit float components.
    Float16x2,
    /// Four 16-bit float components.
    Float16x4,
    /// One 32-bit float component.
    Float32,
    /// Two 32-bit float components.
    Float32x2,
    /// Three 32-bit float components.
    Float32x3,
    /// Four 32-bit float components.
    Float32x4,
    /// One 32-bit unsigned integer component.
    Uint32,
    /// Two 32-bit unsigned integer components.
    Uint32x2,
    /// Three 32-bit unsigned integer components.
    Uint32x3,
    /// Four 32-bit unsigned integer components.
    Uint32x4,
    /// One 32-bit signed integer component.
    Sint32,
    /// Two 32-bit signed integer components.
    Sint32x2,
    /// Three 32-bit signed integer components.
    Sint32x3,
    /// Four 32-bit signed integer components.
    Sint32x4,
}

/// Defines how often the GPU advances to the next element in a vertex buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexStepMode {
    /// The GPU advances to the next element for each vertex.
    Vertex,
    /// The GPU advances to the next element only for each new instance being rendered.
    Instance,
}

/// Defines how vertices are connected to form a geometric primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveTopology {
    /// Vertices are rendered as a list of isolated points.
    PointList,
    /// Vertices are rendered as a list of isolated lines (every two vertices form a line).
    LineList,
    /// Vertices are rendered as a connected line strip.
    LineStrip,
    /// Vertices are rendered as a list of isolated triangles (every three vertices form a triangle).
    TriangleList,
    /// Vertices are rendered as a connected triangle strip.
    TriangleStrip,
}

/// Defines which face of a triangle to cull (not render).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CullMode {
    /// No culling is performed.
    None,
    /// Cull front-facing triangles.
    Front,
    /// Cull back-facing triangles.
    Back,
}

/// Defines which vertex winding order considers a triangle to be "front-facing".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontFace {
    /// Counter-clockwise winding order is the front face (e.g., OpenGL default).
    Ccw,
    /// Clockwise winding order is the front face (e.g., DirectX default).
    Cw,
}

/// Defines how polygons are rasterized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolygonMode {
    /// Polygons are filled. This is the normal rendering mode.
    Fill,
    /// Polygons are rendered as outlines (wireframe).
    Line,
    /// Polygon vertices are rendered as points.
    Point,
}

/// The comparison function used for depth and stencil testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CompareFunction {
    /// The test never passes.
    Never,
    /// The test passes if the new value is less than the existing value.
    Less,
    /// The test passes if the new value is equal to the existing value.
    Equal,
    /// The test passes if the new value is less than or equal to the existing value.
    LessEqual,
    /// The test passes if the new value is greater than the existing value.
    Greater,
    /// The test passes if the new value is not equal to the existing value.
    NotEqual,
    /// The test passes if the new value is greater than or equal to the existing value.
    GreaterEqual,
    /// The test always passes.
    #[default]
    Always,
}

/// An operation to perform on a stencil buffer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StencilOperation {
    /// Keep the existing stencil value.
    #[default]
    Keep,
    /// Set the stencil value to 0.
    Zero,
    /// Replace the stencil value with the reference value.
    Replace,
    /// Bitwise invert the stencil value.
    Invert,
    /// Increment the stencil value, clamping at the maximum value.
    IncrementClamp,
    /// Decrement the stencil value, clamping at 0.
    DecrementClamp,
    /// Increment the stencil value, wrapping to 0 on overflow.
    IncrementWrap,
    /// Decrement the stencil value, wrapping to the maximum value on underflow.
    DecrementWrap,
}

/// A factor in a blend equation, determining how much a source or destination color contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    /// The factor is `0.0`.
    Zero,
    /// The factor is `1.0`.
    One,
    /// The factor is the source alpha component (`src.a`).
    SrcAlpha,
    /// The factor is `1.0 - src.a`.
    OneMinusSrcAlpha,
}

/// The operation used to combine source and destination colors in a blend equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendOperation {
    /// The result is `source + destination`.
    Add,
    /// The result is `source - destination`.
    Subtract,
    /// The result is `destination - source`.
    ReverseSubtract,
    /// The result is `min(source, destination)`.
    Min,
    /// The result is `max(source, destination)`.
    Max,
}

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
    /// The depth bias state.
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

/// Describes the multisampling state for a render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultisampleStateDescriptor {
    /// The number of samples per pixel.
    pub count: SampleCount,
    /// A bitmask where each bit corresponds to a sample. `!0` means all samples are affected.
    pub mask: u32,
    /// If `true`, enables alpha-to-coverage, using the fragment's alpha value to determine coverage.
    pub alpha_to_coverage_enabled: bool,
}

/// A complete descriptor for a render pipeline.
///
/// This struct aggregates all the state needed by the GPU to render primitives.
#[derive(Debug, Clone)]
pub struct RenderPipelineDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The compiled vertex shader module.
    pub vertex_shader_module: ShaderModuleId,
    /// The name of the entry point function in the vertex shader.
    pub vertex_entry_point: Cow<'a, str>,
    /// The compiled fragment shader module, if any.
    pub fragment_shader_module: Option<ShaderModuleId>,
    /// The name of the entry point function in the fragment shader.
    pub fragment_entry_point: Option<Cow<'a, str>>,
    /// The layout of the vertex buffers.
    pub vertex_buffers_layout: Cow<'a, [VertexBufferLayoutDescriptor<'a>]>,
    /// The state for primitive assembly and rasterization.
    pub primitive_state: PrimitiveStateDescriptor,
    /// The state for depth and stencil testing. If `None`, these tests are disabled.
    pub depth_stencil_state: Option<DepthStencilStateDescriptor>,
    /// The states of all color targets this pipeline will render to.
    pub color_target_states: Cow<'a, [ColorTargetStateDescriptor]>,
    /// The multisampling state.
    pub multisample_state: MultisampleStateDescriptor,
}

/// An opaque handle to a compiled render pipeline state object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderPipelineId(pub usize);

/// An opaque handle to a pipeline layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PipelineLayoutId(pub usize);

/// A descriptor for a [`PipelineLayoutId`].
/// Defines the set of resource bindings (e.g., uniform buffers, textures) a pipeline can access.
#[derive(Debug, Clone, Default)]
pub struct PipelineLayoutDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// Bind group layouts will be added here in the future.
    pub _marker: std::marker::PhantomData<&'a ()>,
}

#[cfg(test)]
mod tests {
    use super::{ColorWrites, PrimitiveStateDescriptor, PrimitiveTopology};

    #[test]
    fn test_default_primitive_state() {
        let default_state = PrimitiveStateDescriptor::default();
        assert_eq!(default_state.topology, PrimitiveTopology::TriangleList);
        assert_eq!(default_state.cull_mode, None);
    }

    #[test]
    fn test_color_writes_all() {
        let all_color_writes = ColorWrites::ALL;
        assert_eq!(
            all_color_writes,
            ColorWrites::R | ColorWrites::G | ColorWrites::B | ColorWrites::A
        );
    }
}
