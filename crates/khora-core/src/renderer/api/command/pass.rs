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

//! Descriptors and types for render and compute passes.

use crate::math::LinearRgba;
use crate::renderer::api::core::gpu_hook::GpuHook;
use crate::renderer::api::resource::TextureViewId;

/// Describes the operation to perform on an attachment at the start of a render pass.
#[derive(Clone, Debug)]
pub enum LoadOp<V> {
    /// The existing contents of the attachment will be loaded into the pass.
    Load,
    /// The attachment will be cleared to the specified value before the pass begins.
    Clear(V),
}

/// Describes the operation to perform on an attachment at the end of a render pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreOp {
    /// The results of the render pass will be stored to the attachment's memory.
    Store,
    /// The results of the render pass will be discarded, leaving the attachment's memory undefined.
    Discard,
}

/// Defines the load and store operations for a single render pass attachment.
#[derive(Debug)]
pub struct Operations<V> {
    /// The operation to perform at the beginning of the pass.
    pub load: LoadOp<V>,
    /// The operation to perform at the end of the pass.
    pub store: StoreOp,
}

/// A comprehensive description of a single color attachment for a render pass.
#[derive(Debug)]
pub struct RenderPassColorAttachment<'a> {
    /// The [`TextureViewId`] that will be rendered to.
    pub view: &'a TextureViewId,
    /// If multisampling is used, this is the [`TextureViewId`] that will receive the
    /// resolved (anti-aliased) output. This must be `None` if the `view` is not multisampled.
    pub resolve_target: Option<&'a TextureViewId>,
    /// The load and store operations for this color attachment.
    pub ops: Operations<LinearRgba>,
    /// The target array layer to render to. Defaults to 0.
    pub base_array_layer: u32,
}

/// A comprehensive description of a depth/stencil attachment for a render pass.
#[derive(Debug)]
pub struct RenderPassDepthStencilAttachment<'a> {
    /// The [`TextureViewId`] for the depth/stencil texture.
    pub view: &'a TextureViewId,
    /// The load and store operations for the depth aspect.
    pub depth_ops: Option<Operations<f32>>,
    /// The load and store operations for the stencil aspect.
    pub stencil_ops: Option<Operations<u32>>,
    /// The target array layer to render to. Defaults to 0.
    pub base_array_layer: u32,
}

/// A descriptor for a render pass.
#[derive(Debug, Default)]
pub struct RenderPassDescriptor<'a> {
    /// An optional debug label for the render pass.
    pub label: Option<&'a str>,
    /// A slice of color attachments to be used in the pass.
    pub color_attachments: &'a [RenderPassColorAttachment<'a>],
    /// An optional depth/stencil attachment for this pass.
    pub depth_stencil_attachment: Option<RenderPassDepthStencilAttachment<'a>>,
}

/// Describes a request to write a timestamp at specific points within a pass.
#[derive(Debug, Default)]
pub struct PassTimestampWrites<'a> {
    /// The abstract hook representing the timestamp to be recorded at the beginning of the pass.
    pub beginning_of_pass_hook: Option<&'a GpuHook>,
    /// The abstract hook representing the timestamp to be recorded at the end of the pass.
    pub end_of_pass_hook: Option<&'a GpuHook>,
}

/// A descriptor for a compute pass.
#[derive(Debug, Default)]
pub struct ComputePassDescriptor<'a> {
    /// An optional debug label for the compute pass.
    pub label: Option<&'a str>,
    /// Optional timestamp recording requests for this pass, used for profiling.
    pub timestamp_writes: Option<PassTimestampWrites<'a>>,
}
