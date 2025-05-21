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

use crate::math::{Extent2D, Extent3D, Origin3D};
use crate::subsystems::renderer::api::common_types::{
    SampleCount, ShaderStage, TextureFormat, IndexFormat
};
use crate::subsystems::renderer::api::texture_types::{
    AddressMode, FilterMode, ImageAspect, SamplerBorderColor, TextureDimension,
    TextureViewDimension,
};
use crate::subsystems::renderer::api::pipeline_types::{self as api_pipe};

// --- Dimensions and Origins ---

impl From<Extent2D> for winit::dpi::PhysicalSize<u32> {
    fn from(extent: Extent2D) -> Self {
        winit::dpi::PhysicalSize::new(extent.width, extent.height)
    }
}

// For wgpu::Extent3d conversion
impl From<Extent3D> for wgpu::Extent3d {
    fn from(extent: Extent3D) -> Self {
        wgpu::Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: extent.depth_or_array_layers,
        }
    }
}

impl From<Origin3D> for wgpu::Origin3d {
    fn from(origin: Origin3D) -> Self {
        wgpu::Origin3d {
            x: origin.x,
            y: origin.y,
            z: origin.z,
        }
    }
}

// --- Texture related Enums ---

impl From<TextureDimension> for wgpu::TextureDimension {
    fn from(dim: TextureDimension) -> Self {
        match dim {
            TextureDimension::D1 => wgpu::TextureDimension::D1,
            TextureDimension::D2 => wgpu::TextureDimension::D2,
            TextureDimension::D3 => wgpu::TextureDimension::D3,
        }
    }
}

impl From<TextureViewDimension> for wgpu::TextureViewDimension {
    fn from(dim: TextureViewDimension) -> Self {
        match dim {
            TextureViewDimension::D1 => wgpu::TextureViewDimension::D1,
            TextureViewDimension::D2 => wgpu::TextureViewDimension::D2,
            TextureViewDimension::D2Array => wgpu::TextureViewDimension::D2Array,
            TextureViewDimension::Cube => wgpu::TextureViewDimension::Cube,
            TextureViewDimension::CubeArray => wgpu::TextureViewDimension::CubeArray,
            TextureViewDimension::D3 => wgpu::TextureViewDimension::D3,
        }
    }
}

impl From<ImageAspect> for wgpu::TextureAspect {
    fn from(aspect: ImageAspect) -> Self {
        match aspect {
            ImageAspect::All => wgpu::TextureAspect::All,
            ImageAspect::StencilOnly => wgpu::TextureAspect::StencilOnly,
            ImageAspect::DepthOnly => wgpu::TextureAspect::DepthOnly,
        }
    }
}

impl From<AddressMode> for wgpu::AddressMode {
    fn from(mode: AddressMode) -> Self {
        match mode {
            AddressMode::Repeat => wgpu::AddressMode::Repeat,
            AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
            AddressMode::ClampToBorder => wgpu::AddressMode::ClampToBorder,
        }
    }
}

impl From<FilterMode> for wgpu::FilterMode {
    fn from(mode: FilterMode) -> Self {
        match mode {
            FilterMode::Nearest => wgpu::FilterMode::Nearest,
            FilterMode::Linear => wgpu::FilterMode::Linear,
        }
    }
}

impl From<SamplerBorderColor> for wgpu::SamplerBorderColor {
    fn from(color: SamplerBorderColor) -> Self {
        match color {
            SamplerBorderColor::TransparentBlack => wgpu::SamplerBorderColor::TransparentBlack,
            SamplerBorderColor::OpaqueBlack => wgpu::SamplerBorderColor::OpaqueBlack,
            SamplerBorderColor::OpaqueWhite => wgpu::SamplerBorderColor::OpaqueWhite,
        }
    }
}

// --- Common Types ---

impl From<api_pipe::CompareFunction> for wgpu::CompareFunction {
    fn from(func: api_pipe::CompareFunction) -> Self {
        match func {
            api_pipe::CompareFunction::Never => wgpu::CompareFunction::Never,
            api_pipe::CompareFunction::Less => wgpu::CompareFunction::Less,
            api_pipe::CompareFunction::Equal => wgpu::CompareFunction::Equal,
            api_pipe::CompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
            api_pipe::CompareFunction::Greater => wgpu::CompareFunction::Greater,
            api_pipe::CompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
            api_pipe::CompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            api_pipe::CompareFunction::Always => wgpu::CompareFunction::Always,
        }
    }
}

impl From<TextureFormat> for wgpu::TextureFormat {
    fn from(format: TextureFormat) -> Self {
        match format {
            TextureFormat::R8Unorm => wgpu::TextureFormat::R8Unorm,
            TextureFormat::Rg8Unorm => wgpu::TextureFormat::Rg8Unorm,
            TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
            TextureFormat::R16Float => wgpu::TextureFormat::R16Float,
            TextureFormat::Rg16Float => wgpu::TextureFormat::Rg16Float,
            TextureFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
            TextureFormat::R32Float => wgpu::TextureFormat::R32Float,
            TextureFormat::Rg32Float => wgpu::TextureFormat::Rg32Float,
            TextureFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
            TextureFormat::Depth16Unorm => wgpu::TextureFormat::Depth16Unorm,
            TextureFormat::Depth24Plus => wgpu::TextureFormat::Depth24Plus,
            TextureFormat::Depth24PlusStencil8 => wgpu::TextureFormat::Depth24PlusStencil8,
            TextureFormat::Depth32Float => wgpu::TextureFormat::Depth32Float,
            TextureFormat::Depth32FloatStencil8 => wgpu::TextureFormat::Depth32FloatStencil8,
        }
    }
}

impl From<SampleCount> for u32 {
    fn from(count: SampleCount) -> Self {
        match count {
            SampleCount::X1 => 1,
            SampleCount::X2 => 2,
            SampleCount::X4 => 4,
            SampleCount::X8 => 8,
            SampleCount::X16 => 16,
            SampleCount::X32 => 32,
            SampleCount::X64 => 64,
        }
    }
}

// --- Pipeline related Enums ---

impl From<ShaderStage> for wgpu::ShaderStages {
    fn from(stage: ShaderStage) -> Self {
        match stage {
            ShaderStage::Vertex => wgpu::ShaderStages::VERTEX,
            ShaderStage::Fragment => wgpu::ShaderStages::FRAGMENT,
            ShaderStage::Compute => wgpu::ShaderStages::COMPUTE,
        }
    }
}

impl From<api_pipe::VertexFormat> for wgpu::VertexFormat {
    fn from(format: api_pipe::VertexFormat) -> Self {
        match format {
            api_pipe::VertexFormat::Uint8x2 => wgpu::VertexFormat::Uint8x2,
            api_pipe::VertexFormat::Uint8x4 => wgpu::VertexFormat::Uint8x4,
            api_pipe::VertexFormat::Sint8x2 => wgpu::VertexFormat::Sint8x2,
            api_pipe::VertexFormat::Sint8x4 => wgpu::VertexFormat::Sint8x4,
            api_pipe::VertexFormat::Unorm8x2 => wgpu::VertexFormat::Unorm8x2,
            api_pipe::VertexFormat::Unorm8x4 => wgpu::VertexFormat::Unorm8x4,
            api_pipe::VertexFormat::Snorm8x2 => wgpu::VertexFormat::Snorm8x2,
            api_pipe::VertexFormat::Snorm8x4 => wgpu::VertexFormat::Snorm8x4,
            api_pipe::VertexFormat::Uint16x2 => wgpu::VertexFormat::Uint16x2,
            api_pipe::VertexFormat::Uint16x4 => wgpu::VertexFormat::Uint16x4,
            api_pipe::VertexFormat::Sint16x2 => wgpu::VertexFormat::Sint16x2,
            api_pipe::VertexFormat::Sint16x4 => wgpu::VertexFormat::Sint16x4,
            api_pipe::VertexFormat::Unorm16x2 => wgpu::VertexFormat::Unorm16x2,
            api_pipe::VertexFormat::Unorm16x4 => wgpu::VertexFormat::Unorm16x4,
            api_pipe::VertexFormat::Snorm16x2 => wgpu::VertexFormat::Snorm16x2,
            api_pipe::VertexFormat::Snorm16x4 => wgpu::VertexFormat::Snorm16x4,
            api_pipe::VertexFormat::Float16x2 => wgpu::VertexFormat::Float16x2,
            api_pipe::VertexFormat::Float16x4 => wgpu::VertexFormat::Float16x4,
            api_pipe::VertexFormat::Float32 => wgpu::VertexFormat::Float32,
            api_pipe::VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
            api_pipe::VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
            api_pipe::VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
            api_pipe::VertexFormat::Uint32 => wgpu::VertexFormat::Uint32,
            api_pipe::VertexFormat::Uint32x2 => wgpu::VertexFormat::Uint32x2,
            api_pipe::VertexFormat::Uint32x3 => wgpu::VertexFormat::Uint32x3,
            api_pipe::VertexFormat::Uint32x4 => wgpu::VertexFormat::Uint32x4,
            api_pipe::VertexFormat::Sint32 => wgpu::VertexFormat::Sint32,
            api_pipe::VertexFormat::Sint32x2 => wgpu::VertexFormat::Sint32x2,
            api_pipe::VertexFormat::Sint32x3 => wgpu::VertexFormat::Sint32x3,
            api_pipe::VertexFormat::Sint32x4 => wgpu::VertexFormat::Sint32x4,
        }
    }
}

impl From<api_pipe::VertexStepMode> for wgpu::VertexStepMode {
    fn from(mode: api_pipe::VertexStepMode) -> Self {
        match mode {
            api_pipe::VertexStepMode::Vertex => wgpu::VertexStepMode::Vertex,
            api_pipe::VertexStepMode::Instance => wgpu::VertexStepMode::Instance,
        }
    }
}

impl From<api_pipe::PrimitiveTopology> for wgpu::PrimitiveTopology {
    fn from(topology: api_pipe::PrimitiveTopology) -> Self {
        match topology {
            api_pipe::PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
            api_pipe::PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
            api_pipe::PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
            api_pipe::PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
            api_pipe::PrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
        }
    }
}

impl From<api_pipe::FrontFace> for wgpu::FrontFace {
    fn from(face: api_pipe::FrontFace) -> Self {
        match face {
            api_pipe::FrontFace::Ccw => wgpu::FrontFace::Ccw,
            api_pipe::FrontFace::Cw => wgpu::FrontFace::Cw,
        }
    }
}

impl From<api_pipe::CullMode> for Option<wgpu::Face> {
    fn from(mode: api_pipe::CullMode) -> Self {
        match mode {
            api_pipe::CullMode::Front => Some(wgpu::Face::Front),
            api_pipe::CullMode::Back => Some(wgpu::Face::Back),
            api_pipe::CullMode::None => None
        }
    }
}

impl From<api_pipe::CullMode> for wgpu::Face {
    fn from(mode: api_pipe::CullMode) -> Self {
        match mode {
            api_pipe::CullMode::Front => wgpu::Face::Front,
            api_pipe::CullMode::Back => wgpu::Face::Back,
            api_pipe::CullMode::None => wgpu::Face::Front, // Default to front if none specified
        }
    }
}

impl From<Option<wgpu::Face>> for api_pipe::CullMode {
    fn from(mode: Option<wgpu::Face>) -> Self {
        match mode {
            Some(wgpu::Face::Front) => api_pipe::CullMode::Front,
            Some(wgpu::Face::Back) => api_pipe::CullMode::Back,
            None => api_pipe::CullMode::None,
        }
    }
}

impl From<api_pipe::PolygonMode> for wgpu::PolygonMode {
    fn from(mode: api_pipe::PolygonMode) -> Self {
        match mode {
            api_pipe::PolygonMode::Fill => wgpu::PolygonMode::Fill,
            api_pipe::PolygonMode::Line => wgpu::PolygonMode::Line,
            api_pipe::PolygonMode::Point => wgpu::PolygonMode::Point,
        }
    }
}

impl From<IndexFormat> for wgpu::IndexFormat {
    fn from(format: IndexFormat) -> Self {
        match format {
            IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        }
    }
}

impl From<api_pipe::StencilOperation> for wgpu::StencilOperation {
    fn from(op: api_pipe::StencilOperation) -> Self {
        match op {
            api_pipe::StencilOperation::Keep => wgpu::StencilOperation::Keep,
            api_pipe::StencilOperation::Zero => wgpu::StencilOperation::Zero,
            api_pipe::StencilOperation::Replace => wgpu::StencilOperation::Replace,
            api_pipe::StencilOperation::IncrementClamp => wgpu::StencilOperation::IncrementClamp,
            api_pipe::StencilOperation::DecrementClamp => wgpu::StencilOperation::DecrementClamp,
            api_pipe::StencilOperation::Invert => wgpu::StencilOperation::Invert,
            api_pipe::StencilOperation::IncrementWrap => wgpu::StencilOperation::IncrementWrap,
            api_pipe::StencilOperation::DecrementWrap => wgpu::StencilOperation::DecrementWrap,
        }
    }
}

impl From<api_pipe::BlendFactor> for wgpu::BlendFactor {
    fn from(factor: api_pipe::BlendFactor) -> Self {
        match factor {
            api_pipe::BlendFactor::One => wgpu::BlendFactor::One,
            api_pipe::BlendFactor::Zero => wgpu::BlendFactor::Zero,
            api_pipe::BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
            api_pipe::BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        }
    }
}

impl From<api_pipe::BlendOperation> for wgpu::BlendOperation {
    fn from(op: api_pipe::BlendOperation) -> Self {
        match op {
            api_pipe::BlendOperation::Add => wgpu::BlendOperation::Add,
            api_pipe::BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
            api_pipe::BlendOperation::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
            api_pipe::BlendOperation::Min => wgpu::BlendOperation::Min,
            api_pipe::BlendOperation::Max => wgpu::BlendOperation::Max,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::renderer::api::common_types::{
        SampleCount, ShaderStage, TextureFormat, IndexFormat
    };
    use crate::subsystems::renderer::api::texture_types::{
        AddressMode, FilterMode, ImageAspect, SamplerBorderColor, TextureDimension,
        TextureViewDimension,
    };

    #[test]
    fn test_extent2d_to_physical_size() {
        let extent = Extent2D { width: 128, height: 256 };
        let size: winit::dpi::PhysicalSize<u32> = extent.into();
        assert_eq!(size.width, 128);
        assert_eq!(size.height, 256);
    }

    #[test]
    fn test_extent3d_to_wgpu_extent3d() {
        let extent = Extent3D { width: 1, height: 2, depth_or_array_layers: 3 };
        let w: wgpu::Extent3d = extent.into();
        assert_eq!(w.width, 1);
        assert_eq!(w.height, 2);
        assert_eq!(w.depth_or_array_layers, 3);
    }

    #[test]
    fn test_origin3d_to_wgpu_origin3d() {
        let origin = Origin3D { x: 4, y: 5, z: 6 };
        let w: wgpu::Origin3d = origin.into();
        assert_eq!(w.x, 4);
        assert_eq!(w.y, 5);
        assert_eq!(w.z, 6);
    }

    #[test]
    fn test_texture_dimension_conversion() {
        assert_eq!(wgpu::TextureDimension::D1, TextureDimension::D1.into());
        assert_eq!(wgpu::TextureDimension::D2, TextureDimension::D2.into());
        assert_eq!(wgpu::TextureDimension::D3, TextureDimension::D3.into());
    }

    #[test]
    fn test_texture_view_dimension_conversion() {
        assert_eq!(wgpu::TextureViewDimension::D1, TextureViewDimension::D1.into());
        assert_eq!(wgpu::TextureViewDimension::D2, TextureViewDimension::D2.into());
        assert_eq!(wgpu::TextureViewDimension::D2Array, TextureViewDimension::D2Array.into());
        assert_eq!(wgpu::TextureViewDimension::Cube, TextureViewDimension::Cube.into());
        assert_eq!(wgpu::TextureViewDimension::CubeArray, TextureViewDimension::CubeArray.into());
        assert_eq!(wgpu::TextureViewDimension::D3, TextureViewDimension::D3.into());
    }

    #[test]
    fn test_image_aspect_conversion() {
        assert_eq!(wgpu::TextureAspect::All, ImageAspect::All.into());
        assert_eq!(wgpu::TextureAspect::StencilOnly, ImageAspect::StencilOnly.into());
        assert_eq!(wgpu::TextureAspect::DepthOnly, ImageAspect::DepthOnly.into());
    }

    #[test]
    fn test_address_mode_conversion() {
        assert_eq!(wgpu::AddressMode::Repeat, AddressMode::Repeat.into());
        assert_eq!(wgpu::AddressMode::ClampToEdge, AddressMode::ClampToEdge.into());
        assert_eq!(wgpu::AddressMode::MirrorRepeat, AddressMode::MirrorRepeat.into());
        assert_eq!(wgpu::AddressMode::ClampToBorder, AddressMode::ClampToBorder.into());
    }

    #[test]
    fn test_filter_mode_conversion() {
        assert_eq!(wgpu::FilterMode::Nearest, FilterMode::Nearest.into());
        assert_eq!(wgpu::FilterMode::Linear, FilterMode::Linear.into());
    }

    #[test]
    fn test_sampler_border_color_conversion() {
        assert_eq!(wgpu::SamplerBorderColor::TransparentBlack, SamplerBorderColor::TransparentBlack.into());
        assert_eq!(wgpu::SamplerBorderColor::OpaqueBlack, SamplerBorderColor::OpaqueBlack.into());
        assert_eq!(wgpu::SamplerBorderColor::OpaqueWhite, SamplerBorderColor::OpaqueWhite.into());
    }

    #[test]
    fn test_compare_function_conversion() {
        assert_eq!(wgpu::CompareFunction::Never, api_pipe::CompareFunction::Never.into());
        assert_eq!(wgpu::CompareFunction::Less, api_pipe::CompareFunction::Less.into());
        assert_eq!(wgpu::CompareFunction::Equal, api_pipe::CompareFunction::Equal.into());
        assert_eq!(wgpu::CompareFunction::LessEqual, api_pipe::CompareFunction::LessEqual.into());
        assert_eq!(wgpu::CompareFunction::Greater, api_pipe::CompareFunction::Greater.into());
        assert_eq!(wgpu::CompareFunction::NotEqual, api_pipe::CompareFunction::NotEqual.into());
        assert_eq!(wgpu::CompareFunction::GreaterEqual, api_pipe::CompareFunction::GreaterEqual.into());
        assert_eq!(wgpu::CompareFunction::Always, api_pipe::CompareFunction::Always.into());
    }

    #[test]
    fn test_texture_format_conversion() {
        assert_eq!(wgpu::TextureFormat::R8Unorm, TextureFormat::R8Unorm.into());
        assert_eq!(wgpu::TextureFormat::Rgba8UnormSrgb, TextureFormat::Rgba8UnormSrgb.into());
        assert_eq!(wgpu::TextureFormat::Depth32Float, TextureFormat::Depth32Float.into());
    }

    #[test]
    fn test_sample_count_conversion() {
        assert_eq!(1u32, SampleCount::X1.into());
        assert_eq!(2u32, SampleCount::X2.into());
        assert_eq!(4u32, SampleCount::X4.into());
        assert_eq!(8u32, SampleCount::X8.into());
        assert_eq!(16u32, SampleCount::X16.into());
        assert_eq!(32u32, SampleCount::X32.into());
        assert_eq!(64u32, SampleCount::X64.into());
    }

    #[test]
    fn test_shader_stage_conversion() {
        assert_eq!(wgpu::ShaderStages::VERTEX, ShaderStage::Vertex.into());
        assert_eq!(wgpu::ShaderStages::FRAGMENT, ShaderStage::Fragment.into());
        assert_eq!(wgpu::ShaderStages::COMPUTE, ShaderStage::Compute.into());
    }

    #[test]
    fn test_vertex_format_conversion() {
        assert_eq!(wgpu::VertexFormat::Uint8x2, api_pipe::VertexFormat::Uint8x2.into());
        assert_eq!(wgpu::VertexFormat::Float32x4, api_pipe::VertexFormat::Float32x4.into());
        assert_eq!(wgpu::VertexFormat::Sint32x4, api_pipe::VertexFormat::Sint32x4.into());
    }

    #[test]
    fn test_vertex_step_mode_conversion() {
        assert_eq!(wgpu::VertexStepMode::Vertex, api_pipe::VertexStepMode::Vertex.into());
        assert_eq!(wgpu::VertexStepMode::Instance, api_pipe::VertexStepMode::Instance.into());
    }

    #[test]
    fn test_primitive_topology_conversion() {
        assert_eq!(wgpu::PrimitiveTopology::PointList, api_pipe::PrimitiveTopology::PointList.into());
        assert_eq!(wgpu::PrimitiveTopology::TriangleStrip, api_pipe::PrimitiveTopology::TriangleStrip.into());
    }

    #[test]
    fn test_front_face_conversion() {
        assert_eq!(wgpu::FrontFace::Ccw, api_pipe::FrontFace::Ccw.into());
        assert_eq!(wgpu::FrontFace::Cw, api_pipe::FrontFace::Cw.into());
    }

    #[test]
    fn test_cull_mode_conversion() {
        assert_eq!(Some(wgpu::Face::Front), api_pipe::CullMode::Front.into());
        assert_eq!(Some(wgpu::Face::Back), api_pipe::CullMode::Back.into());
        assert_eq!(None, Into::<Option<wgpu::Face>>::into(api_pipe::CullMode::None));

        assert_eq!(wgpu::Face::Front, api_pipe::CullMode::Front.into());
        assert_eq!(wgpu::Face::Back, api_pipe::CullMode::Back.into());
        assert_eq!(wgpu::Face::Front, api_pipe::CullMode::None.into());

        assert_eq!(api_pipe::CullMode::Front, Some(wgpu::Face::Front).into());
        assert_eq!(api_pipe::CullMode::Back, Some(wgpu::Face::Back).into());
        assert_eq!(api_pipe::CullMode::None, None.into());
    }

    #[test]
    fn test_polygon_mode_conversion() {
        assert_eq!(wgpu::PolygonMode::Fill, api_pipe::PolygonMode::Fill.into());
        assert_eq!(wgpu::PolygonMode::Line, api_pipe::PolygonMode::Line.into());
        assert_eq!(wgpu::PolygonMode::Point, api_pipe::PolygonMode::Point.into());
    }

    #[test]
    fn test_index_format_conversion() {
        assert_eq!(wgpu::IndexFormat::Uint16, IndexFormat::Uint16.into());
        assert_eq!(wgpu::IndexFormat::Uint32, IndexFormat::Uint32.into());
    }

    #[test]
    fn test_stencil_operation_conversion() {
        assert_eq!(wgpu::StencilOperation::Keep, api_pipe::StencilOperation::Keep.into());
        assert_eq!(wgpu::StencilOperation::Zero, api_pipe::StencilOperation::Zero.into());
        assert_eq!(wgpu::StencilOperation::Replace, api_pipe::StencilOperation::Replace.into());
        assert_eq!(wgpu::StencilOperation::IncrementClamp, api_pipe::StencilOperation::IncrementClamp.into());
        assert_eq!(wgpu::StencilOperation::DecrementClamp, api_pipe::StencilOperation::DecrementClamp.into());
        assert_eq!(wgpu::StencilOperation::Invert, api_pipe::StencilOperation::Invert.into());
        assert_eq!(wgpu::StencilOperation::IncrementWrap, api_pipe::StencilOperation::IncrementWrap.into());
        assert_eq!(wgpu::StencilOperation::DecrementWrap, api_pipe::StencilOperation::DecrementWrap.into());
    }

    #[test]
    fn test_blend_factor_conversion() {
        assert_eq!(wgpu::BlendFactor::One, api_pipe::BlendFactor::One.into());
        assert_eq!(wgpu::BlendFactor::Zero, api_pipe::BlendFactor::Zero.into());
        assert_eq!(wgpu::BlendFactor::SrcAlpha, api_pipe::BlendFactor::SrcAlpha.into());
        assert_eq!(wgpu::BlendFactor::OneMinusSrcAlpha, api_pipe::BlendFactor::OneMinusSrcAlpha.into());
    }

    #[test]
    fn test_blend_operation_conversion() {
        assert_eq!(wgpu::BlendOperation::Add, api_pipe::BlendOperation::Add.into());
        assert_eq!(wgpu::BlendOperation::Subtract, api_pipe::BlendOperation::Subtract.into());
        assert_eq!(wgpu::BlendOperation::ReverseSubtract, api_pipe::BlendOperation::ReverseSubtract.into());
        assert_eq!(wgpu::BlendOperation::Min, api_pipe::BlendOperation::Min.into());
        assert_eq!(wgpu::BlendOperation::Max, api_pipe::BlendOperation::Max.into());
    }
}
