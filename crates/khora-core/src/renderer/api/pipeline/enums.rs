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

//! Enums for pipeline configuration.

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

impl VertexFormat {
    /// Returns the size in bytes of this vertex format
    pub fn size(&self) -> usize {
        match self {
            VertexFormat::Float32 => 4,
            VertexFormat::Float32x2 => 8,
            VertexFormat::Float32x3 => 12,
            VertexFormat::Float32x4 => 16,
            VertexFormat::Uint32 => 4,
            VertexFormat::Uint32x2 => 8,
            VertexFormat::Uint32x3 => 12,
            VertexFormat::Uint32x4 => 16,
            VertexFormat::Sint32 => 4,
            VertexFormat::Sint32x2 => 8,
            VertexFormat::Sint32x3 => 12,
            VertexFormat::Sint32x4 => 16,
            VertexFormat::Uint8x2 => 2,
            VertexFormat::Uint8x4 => 4,
            VertexFormat::Sint8x2 => 2,
            VertexFormat::Sint8x4 => 4,
            VertexFormat::Unorm8x2 => 2,
            VertexFormat::Unorm8x4 => 4,
            VertexFormat::Snorm8x2 => 2,
            VertexFormat::Snorm8x4 => 4,
            VertexFormat::Uint16x2 => 4,
            VertexFormat::Uint16x4 => 8,
            VertexFormat::Sint16x2 => 4,
            VertexFormat::Sint16x4 => 8,
            VertexFormat::Unorm16x2 => 4,
            VertexFormat::Unorm16x4 => 8,
            VertexFormat::Snorm16x2 => 4,
            VertexFormat::Snorm16x4 => 8,
            VertexFormat::Float16x2 => 4,
            VertexFormat::Float16x4 => 8,
        }
    }
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
