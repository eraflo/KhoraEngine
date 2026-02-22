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

use wgpu;
use winit;

use khora_core::math::{Extent2D, Extent3D, LinearRgba, Origin3D};
use khora_core::renderer::api::command::bind_group::{
    SamplerBindingType, TextureSampleType,
};
use khora_core::renderer::api::command::pass::{LoadOp, StoreOp};
use khora_core::renderer::api::pipeline::enums::{
    BlendFactor, BlendOperation, CompareFunction, CullMode, FrontFace, PolygonMode,
    PrimitiveTopology, StencilOperation, VertexFormat, VertexStepMode,
};
use khora_core::renderer::api::resource::buffer::BufferUsage;
use khora_core::renderer::api::resource::texture::{
    AddressMode, FilterMode, ImageAspect, MipmapFilterMode, SamplerBorderColor, TextureDimension,
    TextureViewDimension,
};
use khora_core::renderer::api::util::enums::{
    IndexFormat, SampleCount, ShaderStage, TextureFormat,
};
use khora_core::renderer::api::util::flags::ShaderStageFlags;

/// A local extension trait to convert our engine's types into WGPU-compatible types.
/// This avoids Rust's orphan rules while keeping an idiomatic `.into_wgpu()` syntax.
pub trait IntoWgpu<T> {
    /// Consumes self and converts it into a WGPU-compatible type.
    fn into_wgpu(self) -> T;
}

// --- Dimensions and Origins ---

impl IntoWgpu<winit::dpi::PhysicalSize<u32>> for Extent2D {
    fn into_wgpu(self) -> winit::dpi::PhysicalSize<u32> {
        winit::dpi::PhysicalSize::new(self.width, self.height)
    }
}

impl IntoWgpu<wgpu::Extent3d> for Extent3D {
    fn into_wgpu(self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: self.depth_or_array_layers,
        }
    }
}

impl IntoWgpu<wgpu::Origin3d> for Origin3D {
    fn into_wgpu(self) -> wgpu::Origin3d {
        wgpu::Origin3d {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

// --- Texture related Enums ---

impl IntoWgpu<wgpu::TextureDimension> for TextureDimension {
    fn into_wgpu(self) -> wgpu::TextureDimension {
        match self {
            TextureDimension::D1 => wgpu::TextureDimension::D1,
            TextureDimension::D2 => wgpu::TextureDimension::D2,
            TextureDimension::D3 => wgpu::TextureDimension::D3,
        }
    }
}

impl IntoWgpu<wgpu::TextureViewDimension> for TextureViewDimension {
    fn into_wgpu(self) -> wgpu::TextureViewDimension {
        match self {
            TextureViewDimension::D1 => wgpu::TextureViewDimension::D1,
            TextureViewDimension::D2 => wgpu::TextureViewDimension::D2,
            TextureViewDimension::D2Array => wgpu::TextureViewDimension::D2Array,
            TextureViewDimension::Cube => wgpu::TextureViewDimension::Cube,
            TextureViewDimension::CubeArray => wgpu::TextureViewDimension::CubeArray,
            TextureViewDimension::D3 => wgpu::TextureViewDimension::D3,
        }
    }
}

/// IntoWgpu for bind_group's TextureViewDimension (used in BindingType::Texture).
impl IntoWgpu<wgpu::TextureViewDimension>
    for khora_core::renderer::api::command::TextureViewDimension
{
    fn into_wgpu(self) -> wgpu::TextureViewDimension {
        use khora_core::renderer::api::command::TextureViewDimension as CmdTVD;
        match self {
            CmdTVD::D1 => wgpu::TextureViewDimension::D1,
            CmdTVD::D2 => wgpu::TextureViewDimension::D2,
            CmdTVD::D2Array => wgpu::TextureViewDimension::D2Array,
            CmdTVD::Cube => wgpu::TextureViewDimension::Cube,
            CmdTVD::CubeArray => wgpu::TextureViewDimension::CubeArray,
            CmdTVD::D3 => wgpu::TextureViewDimension::D3,
        }
    }
}

impl IntoWgpu<wgpu::TextureSampleType> for TextureSampleType {
    fn into_wgpu(self) -> wgpu::TextureSampleType {
        match self {
            TextureSampleType::Float { filterable } => {
                wgpu::TextureSampleType::Float { filterable }
            }
            TextureSampleType::Depth => wgpu::TextureSampleType::Depth,
            TextureSampleType::Uint => wgpu::TextureSampleType::Uint,
            TextureSampleType::Sint => wgpu::TextureSampleType::Sint,
        }
    }
}

impl IntoWgpu<wgpu::SamplerBindingType> for SamplerBindingType {
    fn into_wgpu(self) -> wgpu::SamplerBindingType {
        match self {
            SamplerBindingType::Filtering => wgpu::SamplerBindingType::Filtering,
            SamplerBindingType::NonFiltering => wgpu::SamplerBindingType::NonFiltering,
            SamplerBindingType::Comparison => wgpu::SamplerBindingType::Comparison,
        }
    }
}

impl IntoWgpu<wgpu::TextureAspect> for ImageAspect {
    fn into_wgpu(self) -> wgpu::TextureAspect {
        match self {
            ImageAspect::All => wgpu::TextureAspect::All,
            ImageAspect::StencilOnly => wgpu::TextureAspect::StencilOnly,
            ImageAspect::DepthOnly => wgpu::TextureAspect::DepthOnly,
        }
    }
}

impl IntoWgpu<wgpu::AddressMode> for AddressMode {
    fn into_wgpu(self) -> wgpu::AddressMode {
        match self {
            AddressMode::Repeat => wgpu::AddressMode::Repeat,
            AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
            AddressMode::ClampToBorder => wgpu::AddressMode::ClampToBorder,
        }
    }
}

impl IntoWgpu<wgpu::FilterMode> for FilterMode {
    fn into_wgpu(self) -> wgpu::FilterMode {
        match self {
            FilterMode::Nearest => wgpu::FilterMode::Nearest,
            FilterMode::Linear => wgpu::FilterMode::Linear,
        }
    }
}

impl IntoWgpu<wgpu::MipmapFilterMode> for MipmapFilterMode {
    fn into_wgpu(self) -> wgpu::MipmapFilterMode {
        match self {
            MipmapFilterMode::Nearest => wgpu::MipmapFilterMode::Nearest,
            MipmapFilterMode::Linear => wgpu::MipmapFilterMode::Linear,
        }
    }
}

impl IntoWgpu<Option<wgpu::SamplerBorderColor>> for SamplerBorderColor {
    fn into_wgpu(self) -> Option<wgpu::SamplerBorderColor> {
        match self {
            SamplerBorderColor::TransparentBlack => {
                Some(wgpu::SamplerBorderColor::TransparentBlack)
            }
            SamplerBorderColor::OpaqueBlack => Some(wgpu::SamplerBorderColor::OpaqueBlack),
            SamplerBorderColor::OpaqueWhite => Some(wgpu::SamplerBorderColor::OpaqueWhite),
        }
    }
}

// --- Common Types ---

impl IntoWgpu<wgpu::CompareFunction> for CompareFunction {
    fn into_wgpu(self) -> wgpu::CompareFunction {
        match self {
            CompareFunction::Never => wgpu::CompareFunction::Never,
            CompareFunction::Less => wgpu::CompareFunction::Less,
            CompareFunction::Equal => wgpu::CompareFunction::Equal,
            CompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
            CompareFunction::Greater => wgpu::CompareFunction::Greater,
            CompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
            CompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            CompareFunction::Always => wgpu::CompareFunction::Always,
        }
    }
}

impl IntoWgpu<wgpu::TextureFormat> for TextureFormat {
    fn into_wgpu(self) -> wgpu::TextureFormat {
        match self {
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

/// Converts a WGPU texture format into its Khora equivalent.
/// This is a free function because we cannot implement `From` due to orphan rules.
pub fn from_wgpu_texture_format(format: wgpu::TextureFormat) -> TextureFormat {
    match format {
        wgpu::TextureFormat::R8Unorm => TextureFormat::R8Unorm,
        wgpu::TextureFormat::Rg8Unorm => TextureFormat::Rg8Unorm,
        wgpu::TextureFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::R16Float => TextureFormat::R16Float,
        wgpu::TextureFormat::Rg16Float => TextureFormat::Rg16Float,
        wgpu::TextureFormat::Rgba16Float => TextureFormat::Rgba16Float,
        wgpu::TextureFormat::R32Float => TextureFormat::R32Float,
        wgpu::TextureFormat::Rg32Float => TextureFormat::Rg32Float,
        wgpu::TextureFormat::Rgba32Float => TextureFormat::Rgba32Float,
        wgpu::TextureFormat::Depth16Unorm => TextureFormat::Depth16Unorm,
        wgpu::TextureFormat::Depth24Plus => TextureFormat::Depth24Plus,
        wgpu::TextureFormat::Depth32Float => TextureFormat::Depth32Float,
        wgpu::TextureFormat::Depth24PlusStencil8 => TextureFormat::Depth24PlusStencil8,
        wgpu::TextureFormat::Depth32FloatStencil8 => TextureFormat::Depth32FloatStencil8,
        _ => unimplemented!(
            "Conversion from wgpu::TextureFormat::{:?} to khora::TextureFormat is not implemented",
            format
        ),
    }
}

impl IntoWgpu<u32> for SampleCount {
    fn into_wgpu(self) -> u32 {
        match self {
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

impl IntoWgpu<wgpu::ShaderStages> for ShaderStage {
    fn into_wgpu(self) -> wgpu::ShaderStages {
        match self {
            ShaderStage::Vertex => wgpu::ShaderStages::VERTEX,
            ShaderStage::Fragment => wgpu::ShaderStages::FRAGMENT,
            ShaderStage::Compute => wgpu::ShaderStages::COMPUTE,
        }
    }
}

impl IntoWgpu<wgpu::ShaderStages> for ShaderStageFlags {
    fn into_wgpu(self) -> wgpu::ShaderStages {
        wgpu::ShaderStages::from_bits_truncate(self.bits())
    }
}

impl IntoWgpu<wgpu::VertexFormat> for VertexFormat {
    fn into_wgpu(self) -> wgpu::VertexFormat {
        match self {
            VertexFormat::Uint8x2 => wgpu::VertexFormat::Uint8x2,
            VertexFormat::Uint8x4 => wgpu::VertexFormat::Uint8x4,
            VertexFormat::Sint8x2 => wgpu::VertexFormat::Sint8x2,
            VertexFormat::Sint8x4 => wgpu::VertexFormat::Sint8x4,
            VertexFormat::Unorm8x2 => wgpu::VertexFormat::Unorm8x2,
            VertexFormat::Unorm8x4 => wgpu::VertexFormat::Unorm8x4,
            VertexFormat::Snorm8x2 => wgpu::VertexFormat::Snorm8x2,
            VertexFormat::Snorm8x4 => wgpu::VertexFormat::Snorm8x4,
            VertexFormat::Uint16x2 => wgpu::VertexFormat::Uint16x2,
            VertexFormat::Uint16x4 => wgpu::VertexFormat::Uint16x4,
            VertexFormat::Sint16x2 => wgpu::VertexFormat::Sint16x2,
            VertexFormat::Sint16x4 => wgpu::VertexFormat::Sint16x4,
            VertexFormat::Unorm16x2 => wgpu::VertexFormat::Unorm16x2,
            VertexFormat::Unorm16x4 => wgpu::VertexFormat::Unorm16x4,
            VertexFormat::Snorm16x2 => wgpu::VertexFormat::Snorm16x2,
            VertexFormat::Snorm16x4 => wgpu::VertexFormat::Snorm16x4,
            VertexFormat::Float16x2 => wgpu::VertexFormat::Float16x2,
            VertexFormat::Float16x4 => wgpu::VertexFormat::Float16x4,
            VertexFormat::Float32 => wgpu::VertexFormat::Float32,
            VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
            VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
            VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
            VertexFormat::Uint32 => wgpu::VertexFormat::Uint32,
            VertexFormat::Uint32x2 => wgpu::VertexFormat::Uint32x2,
            VertexFormat::Uint32x3 => wgpu::VertexFormat::Uint32x3,
            VertexFormat::Uint32x4 => wgpu::VertexFormat::Uint32x4,
            VertexFormat::Sint32 => wgpu::VertexFormat::Sint32,
            VertexFormat::Sint32x2 => wgpu::VertexFormat::Sint32x2,
            VertexFormat::Sint32x3 => wgpu::VertexFormat::Sint32x3,
            VertexFormat::Sint32x4 => wgpu::VertexFormat::Sint32x4,
        }
    }
}

impl IntoWgpu<wgpu::VertexStepMode> for VertexStepMode {
    fn into_wgpu(self) -> wgpu::VertexStepMode {
        match self {
            VertexStepMode::Vertex => wgpu::VertexStepMode::Vertex,
            VertexStepMode::Instance => wgpu::VertexStepMode::Instance,
        }
    }
}

impl IntoWgpu<wgpu::PrimitiveTopology> for PrimitiveTopology {
    fn into_wgpu(self) -> wgpu::PrimitiveTopology {
        match self {
            PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
            PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
            PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
            PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
            PrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
        }
    }
}

impl IntoWgpu<wgpu::FrontFace> for FrontFace {
    fn into_wgpu(self) -> wgpu::FrontFace {
        match self {
            FrontFace::Ccw => wgpu::FrontFace::Ccw,
            FrontFace::Cw => wgpu::FrontFace::Cw,
        }
    }
}

impl IntoWgpu<Option<wgpu::Face>> for CullMode {
    fn into_wgpu(self) -> Option<wgpu::Face> {
        match self {
            CullMode::Front => Some(wgpu::Face::Front),
            CullMode::Back => Some(wgpu::Face::Back),
            CullMode::None => None,
        }
    }
}

impl IntoWgpu<wgpu::PolygonMode> for PolygonMode {
    fn into_wgpu(self) -> wgpu::PolygonMode {
        match self {
            PolygonMode::Fill => wgpu::PolygonMode::Fill,
            PolygonMode::Line => wgpu::PolygonMode::Line,
            PolygonMode::Point => wgpu::PolygonMode::Point,
        }
    }
}

impl IntoWgpu<wgpu::IndexFormat> for IndexFormat {
    fn into_wgpu(self) -> wgpu::IndexFormat {
        match self {
            IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        }
    }
}

impl IntoWgpu<wgpu::StencilOperation> for StencilOperation {
    fn into_wgpu(self) -> wgpu::StencilOperation {
        match self {
            StencilOperation::Keep => wgpu::StencilOperation::Keep,
            StencilOperation::Zero => wgpu::StencilOperation::Zero,
            StencilOperation::Replace => wgpu::StencilOperation::Replace,
            StencilOperation::IncrementClamp => wgpu::StencilOperation::IncrementClamp,
            StencilOperation::DecrementClamp => wgpu::StencilOperation::DecrementClamp,
            StencilOperation::Invert => wgpu::StencilOperation::Invert,
            StencilOperation::IncrementWrap => wgpu::StencilOperation::IncrementWrap,
            StencilOperation::DecrementWrap => wgpu::StencilOperation::DecrementWrap,
        }
    }
}

impl IntoWgpu<wgpu::BlendFactor> for BlendFactor {
    fn into_wgpu(self) -> wgpu::BlendFactor {
        match self {
            BlendFactor::One => wgpu::BlendFactor::One,
            BlendFactor::Zero => wgpu::BlendFactor::Zero,
            BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        }
    }
}

impl IntoWgpu<wgpu::BlendOperation> for BlendOperation {
    fn into_wgpu(self) -> wgpu::BlendOperation {
        match self {
            BlendOperation::Add => wgpu::BlendOperation::Add,
            BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
            BlendOperation::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
            BlendOperation::Min => wgpu::BlendOperation::Min,
            BlendOperation::Max => wgpu::BlendOperation::Max,
        }
    }
}

impl IntoWgpu<wgpu::BufferUsages> for BufferUsage {
    fn into_wgpu(self) -> wgpu::BufferUsages {
        let mut usages = wgpu::BufferUsages::empty();
        if self.contains(BufferUsage::COPY_SRC) {
            usages |= wgpu::BufferUsages::COPY_SRC;
        }
        if self.contains(BufferUsage::COPY_DST) {
            usages |= wgpu::BufferUsages::COPY_DST;
        }
        if self.contains(BufferUsage::INDEX) {
            usages |= wgpu::BufferUsages::INDEX;
        }
        if self.contains(BufferUsage::VERTEX) {
            usages |= wgpu::BufferUsages::VERTEX;
        }
        if self.contains(BufferUsage::UNIFORM) {
            usages |= wgpu::BufferUsages::UNIFORM;
        }
        if self.contains(BufferUsage::STORAGE) {
            usages |= wgpu::BufferUsages::STORAGE;
        }
        if self.contains(BufferUsage::INDIRECT) {
            usages |= wgpu::BufferUsages::INDIRECT;
        }
        usages
    }
}

impl IntoWgpu<wgpu::StoreOp> for StoreOp {
    fn into_wgpu(self) -> wgpu::StoreOp {
        match self {
            StoreOp::Store => wgpu::StoreOp::Store,
            StoreOp::Discard => wgpu::StoreOp::Discard,
        }
    }
}

impl IntoWgpu<wgpu::LoadOp<wgpu::Color>> for LoadOp<LinearRgba> {
    fn into_wgpu(self) -> wgpu::LoadOp<wgpu::Color> {
        match self {
            LoadOp::Load => wgpu::LoadOp::Load,
            LoadOp::Clear(color) => wgpu::LoadOp::Clear(wgpu::Color {
                r: color.r as f64,
                g: color.g as f64,
                b: color.b as f64,
                a: color.a as f64,
            }),
        }
    }
}

impl IntoWgpu<wgpu::LoadOp<f32>> for LoadOp<f32> {
    fn into_wgpu(self) -> wgpu::LoadOp<f32> {
        match self {
            LoadOp::Load => wgpu::LoadOp::Load,
            LoadOp::Clear(depth) => wgpu::LoadOp::Clear(depth),
        }
    }
}

impl IntoWgpu<wgpu::LoadOp<u32>> for LoadOp<u32> {
    fn into_wgpu(self) -> wgpu::LoadOp<u32> {
        match self {
            LoadOp::Load => wgpu::LoadOp::Load,
            LoadOp::Clear(stencil) => wgpu::LoadOp::Clear(stencil),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::{Extent2D, Extent3D, Origin3D};

    #[test]
    fn test_extent2d_to_physical_size() {
        let extent = Extent2D {
            width: 128,
            height: 256,
        };
        let size: winit::dpi::PhysicalSize<u32> = extent.into_wgpu();
        assert_eq!(size.width, 128);
        assert_eq!(size.height, 256);
    }

    #[test]
    fn test_extent3d_to_wgpu_extent3d() {
        let extent = Extent3D {
            width: 1,
            height: 2,
            depth_or_array_layers: 3,
        };
        let w: wgpu::Extent3d = extent.into_wgpu();
        assert_eq!(w.width, 1);
        assert_eq!(w.height, 2);
        assert_eq!(w.depth_or_array_layers, 3);
    }

    #[test]
    fn test_origin3d_to_wgpu_origin3d() {
        let origin = Origin3D { x: 4, y: 5, z: 6 };
        let w: wgpu::Origin3d = origin.into_wgpu();
        assert_eq!(w.x, 4);
        assert_eq!(w.y, 5);
        assert_eq!(w.z, 6);
    }

    #[test]
    fn test_texture_dimension_conversion() {
        assert_eq!(wgpu::TextureDimension::D1, TextureDimension::D1.into_wgpu());
        assert_eq!(wgpu::TextureDimension::D2, TextureDimension::D2.into_wgpu());
        assert_eq!(wgpu::TextureDimension::D3, TextureDimension::D3.into_wgpu());
    }

    #[test]
    fn test_texture_view_dimension_conversion() {
        assert_eq!(
            wgpu::TextureViewDimension::D1,
            TextureViewDimension::D1.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureViewDimension::D2,
            TextureViewDimension::D2.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureViewDimension::D2Array,
            TextureViewDimension::D2Array.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureViewDimension::Cube,
            TextureViewDimension::Cube.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureViewDimension::CubeArray,
            TextureViewDimension::CubeArray.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureViewDimension::D3,
            TextureViewDimension::D3.into_wgpu()
        );
    }

    #[test]
    fn test_image_aspect_conversion() {
        assert_eq!(wgpu::TextureAspect::All, ImageAspect::All.into_wgpu());
        assert_eq!(
            wgpu::TextureAspect::StencilOnly,
            ImageAspect::StencilOnly.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureAspect::DepthOnly,
            ImageAspect::DepthOnly.into_wgpu()
        );
    }

    #[test]
    fn test_address_mode_conversion() {
        assert_eq!(wgpu::AddressMode::Repeat, AddressMode::Repeat.into_wgpu());
        assert_eq!(
            wgpu::AddressMode::ClampToEdge,
            AddressMode::ClampToEdge.into_wgpu()
        );
        assert_eq!(
            wgpu::AddressMode::MirrorRepeat,
            AddressMode::MirrorRepeat.into_wgpu()
        );
        assert_eq!(
            wgpu::AddressMode::ClampToBorder,
            AddressMode::ClampToBorder.into_wgpu()
        );
    }

    #[test]
    fn test_filter_mode_conversion() {
        assert_eq!(wgpu::FilterMode::Nearest, FilterMode::Nearest.into_wgpu());
        assert_eq!(wgpu::FilterMode::Linear, FilterMode::Linear.into_wgpu());
    }

    #[test]
    fn test_sampler_border_color_conversion() {
        assert_eq!(
            Some(wgpu::SamplerBorderColor::TransparentBlack),
            SamplerBorderColor::TransparentBlack.into_wgpu()
        );
        assert_eq!(
            Some(wgpu::SamplerBorderColor::OpaqueBlack),
            SamplerBorderColor::OpaqueBlack.into_wgpu()
        );
        assert_eq!(
            Some(wgpu::SamplerBorderColor::OpaqueWhite),
            SamplerBorderColor::OpaqueWhite.into_wgpu()
        );
    }

    #[test]
    fn test_compare_function_conversion() {
        assert_eq!(
            wgpu::CompareFunction::Never,
            CompareFunction::Never.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::Less,
            CompareFunction::Less.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::Equal,
            CompareFunction::Equal.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::LessEqual,
            CompareFunction::LessEqual.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::Greater,
            CompareFunction::Greater.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::NotEqual,
            CompareFunction::NotEqual.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::GreaterEqual,
            CompareFunction::GreaterEqual.into_wgpu()
        );
        assert_eq!(
            wgpu::CompareFunction::Always,
            CompareFunction::Always.into_wgpu()
        );
    }

    #[test]
    fn test_texture_format_conversion() {
        assert_eq!(
            wgpu::TextureFormat::R8Unorm,
            TextureFormat::R8Unorm.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Rgba8UnormSrgb.into_wgpu()
        );
        assert_eq!(
            wgpu::TextureFormat::Depth32Float,
            TextureFormat::Depth32Float.into_wgpu()
        );
    }

    #[test]
    fn test_sample_count_conversion() {
        assert_eq!(1u32, SampleCount::X1.into_wgpu());
        assert_eq!(2u32, SampleCount::X2.into_wgpu());
        assert_eq!(4u32, SampleCount::X4.into_wgpu());
        assert_eq!(8u32, SampleCount::X8.into_wgpu());
        assert_eq!(16u32, SampleCount::X16.into_wgpu());
    }

    #[test]
    fn test_shader_stage_conversion() {
        assert_eq!(wgpu::ShaderStages::VERTEX, ShaderStage::Vertex.into_wgpu());
        assert_eq!(
            wgpu::ShaderStages::FRAGMENT,
            ShaderStage::Fragment.into_wgpu()
        );
        assert_eq!(
            wgpu::ShaderStages::COMPUTE,
            ShaderStage::Compute.into_wgpu()
        );
    }

    #[test]
    fn test_vertex_format_conversion() {
        assert_eq!(
            wgpu::VertexFormat::Uint8x2,
            VertexFormat::Uint8x2.into_wgpu()
        );
        assert_eq!(
            wgpu::VertexFormat::Float32x4,
            VertexFormat::Float32x4.into_wgpu()
        );
        assert_eq!(
            wgpu::VertexFormat::Sint32x4,
            VertexFormat::Sint32x4.into_wgpu()
        );
    }

    #[test]
    fn test_vertex_step_mode_conversion() {
        assert_eq!(
            wgpu::VertexStepMode::Vertex,
            VertexStepMode::Vertex.into_wgpu()
        );
        assert_eq!(
            wgpu::VertexStepMode::Instance,
            VertexStepMode::Instance.into_wgpu()
        );
    }

    #[test]
    fn test_primitive_topology_conversion() {
        assert_eq!(
            wgpu::PrimitiveTopology::PointList,
            PrimitiveTopology::PointList.into_wgpu()
        );
        assert_eq!(
            wgpu::PrimitiveTopology::TriangleStrip,
            PrimitiveTopology::TriangleStrip.into_wgpu()
        );
    }

    #[test]
    fn test_front_face_conversion() {
        assert_eq!(wgpu::FrontFace::Ccw, FrontFace::Ccw.into_wgpu());
        assert_eq!(wgpu::FrontFace::Cw, FrontFace::Cw.into_wgpu());
    }

    #[test]
    fn test_cull_mode_conversion() {
        let to_option: Option<wgpu::Face> = CullMode::Front.into_wgpu();
        assert_eq!(Some(wgpu::Face::Front), to_option);

        let to_option_back: Option<wgpu::Face> = CullMode::Back.into_wgpu();
        assert_eq!(Some(wgpu::Face::Back), to_option_back);

        let to_option_none: Option<wgpu::Face> = CullMode::None.into_wgpu();
        assert_eq!(None, to_option_none);
    }

    #[test]
    fn test_polygon_mode_conversion() {
        assert_eq!(wgpu::PolygonMode::Fill, PolygonMode::Fill.into_wgpu());
        assert_eq!(wgpu::PolygonMode::Line, PolygonMode::Line.into_wgpu());
        assert_eq!(wgpu::PolygonMode::Point, PolygonMode::Point.into_wgpu());
    }

    #[test]
    fn test_index_format_conversion() {
        assert_eq!(wgpu::IndexFormat::Uint16, IndexFormat::Uint16.into_wgpu());
        assert_eq!(wgpu::IndexFormat::Uint32, IndexFormat::Uint32.into_wgpu());
    }

    #[test]
    fn test_stencil_operation_conversion() {
        assert_eq!(
            wgpu::StencilOperation::Keep,
            StencilOperation::Keep.into_wgpu()
        );
        assert_eq!(
            wgpu::StencilOperation::Zero,
            StencilOperation::Zero.into_wgpu()
        );
    }

    #[test]
    fn test_blend_factor_conversion() {
        assert_eq!(wgpu::BlendFactor::One, BlendFactor::One.into_wgpu());
        assert_eq!(wgpu::BlendFactor::Zero, BlendFactor::Zero.into_wgpu());
    }

    #[test]
    fn test_blend_operation_conversion() {
        assert_eq!(wgpu::BlendOperation::Add, BlendOperation::Add.into_wgpu());
    }
}
