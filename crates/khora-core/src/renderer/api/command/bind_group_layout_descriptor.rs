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

//! The descriptor of a bind group layout, and the binding entries it lists.

use crate::renderer::api::resource::TextureViewDimension;
use crate::renderer::api::util::flags::ShaderStageFlags;

/// Describes a bind group layout to be created.
#[derive(Debug, Clone)]
pub struct BindGroupLayoutDescriptor<'a> {
    /// Optional debug label.
    pub label: Option<&'a str>,
    /// The entries in this bind group layout.
    pub entries: &'a [BindGroupLayoutEntry],
}

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
