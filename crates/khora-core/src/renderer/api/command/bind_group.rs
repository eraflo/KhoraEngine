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

//! Defines data structures for bind groups and bind group layouts.
//!
//! Bind groups are the mechanism for binding resources (buffers, textures, samplers)
//! to shaders in a graphics pipeline. They provide an abstraction over the different
//! binding models of various graphics APIs (descriptor sets in Vulkan, bind groups in WebGPU).

use crate::renderer::api::{
    resource::{BufferId, SamplerId, TextureViewId},
    util::flags::ShaderStageFlags,
};

/// An opaque handle to a bind group layout resource.
///
/// A bind group layout describes the structure and types of resources
/// that will be bound to a shader, without specifying the actual resources themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindGroupLayoutId(pub usize);

/// An opaque handle to a bind group resource.
///
/// A bind group represents the actual bound resources (buffers, textures, etc.)
/// that match a specific bind group layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindGroupId(pub usize);

/// Describes a single binding entry in a bind group layout.
#[derive(Debug, Clone)]
pub struct BindGroupLayoutEntry {
    /// The binding index (e.g., `@binding(0)` in WGSL).
    pub binding: u32,
    /// Which shader stages can access this binding.
    pub visibility: ShaderStageFlags,
    /// The type of resource being bound.
    pub ty: BindingType,
}

impl BindGroupLayoutEntry {
    /// Helper to create a BindGroupLayoutEntry for a buffer resource.
    pub fn buffer(
        binding: u32,
        visibility: ShaderStageFlags,
        ty: BufferBindingType,
        has_dynamic_offset: bool,
        min_binding_size: Option<std::num::NonZeroU64>,
    ) -> Self {
        Self {
            binding,
            visibility,
            ty: BindingType::Buffer {
                ty,
                has_dynamic_offset,
                min_binding_size,
            },
        }
    }
}

/// Describes the type of buffer binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferBindingType {
    /// A uniform buffer.
    Uniform,
    /// A storage buffer (read/write or read-only).
    Storage {
        /// Whether the buffer is read-only in the shader.
        read_only: bool,
    },
}

/// The type of texture view dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureViewDimension {
    /// A 1D texture view.
    D1,
    /// A 2D texture view.
    D2,
    /// A 2D array texture view.
    D2Array,
    /// A cube texture view.
    Cube,
    /// A cube array texture view.
    CubeArray,
    /// A 3D texture view.
    D3,
}

/// The type of texture sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureSampleType {
    /// A floating-point texture sample.
    Float {
        /// Whether the texture can be filtered.
        filterable: bool,
    },
    /// A depth texture sample.
    Depth,
    /// An unsigned integer texture sample.
    Uint,
    /// A signed integer texture sample.
    Sint,
}

/// The type of sampler binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerBindingType {
    /// A filtering sampler.
    Filtering,
    /// A non-filtering sampler.
    NonFiltering,
    /// A comparison sampler.
    Comparison,
}

/// The type of resource bound at a binding point.
#[derive(Debug, Clone)]
pub enum BindingType {
    /// A buffer binding (uniform or storage).
    Buffer {
        /// The type of buffer binding.
        ty: BufferBindingType,
        /// Whether this buffer has dynamic offsets.
        has_dynamic_offset: bool,
        /// Minimum size required for the buffer binding.
        min_binding_size: Option<std::num::NonZeroU64>,
    },
    /// A sampled texture binding.
    Texture {
        /// The type of sampler that can sample this texture.
        sample_type: TextureSampleType,
        /// The dimension of the texture view.
        view_dimension: TextureViewDimension,
        /// Whether the texture supports multisampling.
        multisampled: bool,
    },
    /// A sampler binding.
    Sampler(SamplerBindingType),
}

/// Describes a bind group layout to be created.
#[derive(Debug, Clone)]
pub struct BindGroupLayoutDescriptor<'a> {
    /// Optional debug label.
    pub label: Option<&'a str>,
    /// The entries in this bind group layout.
    pub entries: &'a [BindGroupLayoutEntry],
}

/// Describes a buffer binding with offset and size.
#[derive(Debug, Clone, Copy)]
pub struct BufferBinding {
    /// The buffer to bind.
    pub buffer: BufferId,
    /// Offset into the buffer in bytes.
    pub offset: u64,
    /// Size of the binding, or None to bind from offset to end of buffer.
    pub size: Option<std::num::NonZeroU64>,
}

/// Describes a single resource binding in a bind group.
#[derive(Debug, Clone, Copy)]
pub enum BindingResource {
    /// Binds a buffer with optional offset and size.
    Buffer(BufferBinding),
    /// Binds a texture view.
    TextureView(TextureViewId),
    /// Binds a sampler.
    Sampler(SamplerId),
}

/// Describes a bind group to be created.
#[derive(Debug, Clone)]
pub struct BindGroupDescriptor<'a> {
    /// Optional debug label.
    pub label: Option<&'a str>,
    /// The layout this bind group conforms to.
    pub layout: BindGroupLayoutId,
    /// The resources to bind at each binding point.
    pub entries: &'a [BindGroupEntry<'a>],
}

/// A single entry in a bind group.
#[derive(Debug, Clone, Copy)]
pub struct BindGroupEntry<'a> {
    /// The binding index.
    pub binding: u32,
    /// The resource to bind.
    pub resource: BindingResource,
    /// Phantom data to preserve lifetime
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> BindGroupEntry<'a> {
    /// Helper to create a BindGroupEntry for a buffer with default offset (0) and size (None).
    pub fn buffer(
        binding: u32,
        buffer: BufferId,
        offset: u64,
        size: Option<std::num::NonZeroU64>,
    ) -> Self {
        Self {
            binding,
            resource: BindingResource::Buffer(BufferBinding {
                buffer,
                offset,
                size,
            }),
            _phantom: std::marker::PhantomData,
        }
    }
}
