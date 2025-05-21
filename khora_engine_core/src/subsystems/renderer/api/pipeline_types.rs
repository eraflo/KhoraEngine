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

use std::borrow::Cow;

/// Represents the format of vertex attributes in a vertex buffer.
/// This is used to define how vertex data is laid out in memory and how it should be interpreted.
/// The format determines the type of data stored in the vertex buffer, such as floating-point numbers or integers.
/// The format also specifies the number of components per vertex attribute, such as 1, 2, 3, or 4 components.
/// Essential for gpu to understand how to interpret the data in the vertex buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Uint8x2, Uint8x4,
    Sint8x2, Sint8x4,
    Unorm8x2, Unorm8x4,
    Snorm8x2, Snorm8x4,
    Uint16x2, Uint16x4,
    Sint16x2, Sint16x4,
    Unorm16x2, Unorm16x4,
    Snorm16x2, Snorm16x4,
    Float16x2, Float16x4,
    Float32, Float32x2, Float32x3, Float32x4,
    Uint32, Uint32x2, Uint32x3, Uint32x4,
    Sint32, Sint32x2, Sint32x3, Sint32x4
}

/// Represents the step mode for vertex attributes.
/// This determines how to read the vertex data from the vertex buffer.
/// The step mode can be either per-vertex or per-instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexStepMode {
    Vertex, // Read for each vertex
    Instance // Read 1 time for each instance of an object drawn
}

/// Defines how vertices are connected to form primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
}

/// Represents the culling mode for rendering.
/// This determines which faces of a 3D object are rendered and which are culled (not rendered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CullMode {
    None,
    Front,
    Back
}

/// Determines which side of a triangle is considered the front face based on the order of its vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontFace {
    Ccw,
    Cw
}

/// Use for rendering polygons.
/// This determines how polygons are rendered.
/// The polygon mode can be set to fill, line, or point.
/// - `Fill`: The polygon is filled with color.
/// - `Line`: The polygon is rendered as a wireframe.
/// - `Point`: The polygon is rendered as points.
/// This is used to control the appearance of polygons in the rendered scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolygonMode {
    Fill,
    Line,
    Point
}

/// Defines compare conditions for depth and stencil tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always
}

/// Describes action to take on the buffer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StencilOperation {
    Keep,
    Zero,
    Replace,
    Invert,
    IncrementClamp,
    DecrementClamp,
    IncrementWrap,
    DecrementWrap
}

/// Defines multiplier to apply on source color (current fragment) and destination color (already on rendered image) when blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    Zero,
    One,
    SrcAlpha,
    OneMinusSrcAlpha
}

/// Defines operation to apply to combine source and destination colors, pondering on the blend factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendOperation {
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max
}


/// Describes a vertex attribute.
/// This includes the shader location (the index of the attribute in the shader),
/// the format (the type of data stored in the attribute), and the offset (the byte offset from the start of the vertex).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VertexAttributeDescriptor<'a> {
    pub shader_location: u32, // The index of the attribute in the shader
    pub format: VertexFormat, // The format of the attribute data
    pub offset: u64, // The byte offset from the start of the vertex
}


/// Describes a vertex buffer layout.
/// This is used to define how vertex data is organized in memory.
/// The layout includes the stride (the size of each vertex in bytes), the step mode (per-vertex or per-instance),
/// and the attributes (the individual vertex attributes).
#[derive(Debug, Clone)]
pub struct VertexBufferLayoutDescriptor<'a> {
    pub array_stride: u64,
    pub step_mode: VertexStepMode,
    pub attributes: Cow<'a, [VertexAttributeDescriptor]>
}

/// Regroupes all parameters needed to combine vertices in primitives and draw them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrimitiveStateDescriptor {
    pub topology: PrimitiveTopology,
    pub strip_index_format: Option<IndexFormat>,
    pub front_face: FrontFace,
    pub cull_mode: Option<CullMode>,
    pub polygon_mode: PolygonMode,
    pub unclipped_depth: bool, // Use for shadow techniques or volumetric rendering
    pub conservative: bool // Use for gpu collision detection or voxel-tracing
}

/// Defines the behavior of the stencil buffer.
/// Stencil buffer is an auxiliary buffer used for masking pixels during rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StencilFaceState {
    pub compare: CompareFunction,
    pub fail_op: StencilOperation, // Operation to perform if the stencil test fails
    pub depth_fail_op: StencilOperation, // Operation to perform if the stencil test passes but the depth test fails
    pub depth_pass_op: StencilOperation // Operation to perform if both the stencil and depth tests pass
}

/// Use to add offset (bias) to depth value calculated for a primitive.
/// This is used to prevent z-fighting (depth fighting) between two overlapping primitives.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DepthBiasState {
    pub constant: i32, // Constant bias to add to the depth value
    pub slope_scale: f32, // Slope scale bias to add to the depth value based on the slope of the primitive
    pub clamp: f32, // Maximum value to which the depth value can be clamped (no clamp = 0.0)
}

/// Regroups all parameters needed to configure the depth and stencil tests.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthStencilStateDescriptor {
    pub format: TextureFormat,
    pub depth_write_enabled: bool,
    pub depth_compare: CompareFunction,
    pub stencil_front: StencilFaceState,
    pub stencil_back: StencilFaceState,
    pub stencil_read_mask: u32, // Bits of stencil to read
    pub stencil_write_mask: u32, // Bits of stencil to write
    pub bias: DepthBiasState
}

/// Describes a complete blending equation for a color component. 
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendComponentDescriptor {
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
    pub operation: BlendOperation
}

/// Combines the blending equations for color and alpha components (which can be different).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendStateDescriptor {
    pub color: BlendComponentDescriptor,
    pub alpha: BlendComponentDescriptor
}

/// Bitmask to enable or disable color writes for each color channel.
/// This is used to control which color channels are written to the render target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorWrites {
    pub r: bool,
    pub g: bool,
    pub b: bool,
    pub a: bool
}

/// Describes a color target state. A pipeline can have multiple color targets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColorTargetStateDescriptor {
    pub format: TextureFormat,
    pub blend: Option<BlendStateDescriptor>,
    pub write_mask: ColorWrites
}

/// Configure MSAA (multisample anti-aliasing) for the render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultisampleStateDescriptor {
    pub count: u32, // Number of samples per pixel
    pub mask: u32, // Bitmask to select which samples are affected
    pub alpha_to_coverage_enabled: bool // Use alpha to determine coverage for MSAA
}

/// Use to ask gpu to create a render pipeline.
/// Regroups all states and shaders for a rendering pass.
#[derive(Debug, Clone)]
pub struct RenderPipelineDescriptor<'a> {
    pub label: Option<Cow<'a, str>>,
    pub vertex_shader_module: ShaderModuleId, // The shader module used for vertex processing
    pub vertex_entry_point: Cow<'a, str>, // The entry point function in the vertex shader
    pub fragment_shader_module: Option<ShaderModuleId>, // The shader module used for fragment processing (optional)
    pub fragment_entry_point: Option<Cow<'a, str>>, // The entry point function in the fragment shader (optional)
    pub vertex_buffers_layout: Cow<'a, [VertexBufferLayoutDescriptor<'a>]>, // Description of vertex buffer
    pub primitive_state: PrimitiveStateDescriptor,
    pub depth_stencil_state: Option<DepthStencilStateDescriptor>,
    pub color_target_states: Cow<'a, [ColorTargetStateDescriptor]>,
    pub multisample_state: MultisampleStateDescriptor,
}

/// An opaque handle representing a render pipeline.
/// This ID is used to identify a specific render pipeline within the graphics device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderPipelineId(pub usize);