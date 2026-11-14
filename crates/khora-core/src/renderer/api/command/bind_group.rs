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

pub use super::bind_group_id::BindGroupId;
pub use super::bind_group_layout_descriptor::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
    SamplerBindingType, TextureSampleType,
};
pub use crate::renderer::api::resource::TextureViewDimension;
use crate::renderer::api::resource::{BufferId, SamplerId, TextureViewId};

/// An opaque handle to a bind group layout resource.
///
/// A bind group layout describes the structure and types of resources
/// that will be bound to a shader, without specifying the actual resources themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindGroupLayoutId(pub usize);

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
