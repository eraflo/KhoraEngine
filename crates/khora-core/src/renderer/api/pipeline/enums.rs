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

pub use super::blend_factor::BlendFactor;
pub use super::blend_operation::BlendOperation;
pub use super::compare_function::CompareFunction;
pub use super::cull_mode::CullMode;
pub use super::front_face::FrontFace;
pub use super::polygon_mode::PolygonMode;
pub use super::primitive_topology::PrimitiveTopology;
pub use super::stencil_operation::StencilOperation;
pub use super::vertex_step_mode::VertexStepMode;

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
