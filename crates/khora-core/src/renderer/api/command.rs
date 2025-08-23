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

//! Defines data structures used for recording and describing GPU commands.

use crate::math::LinearRgba;
use crate::renderer::{GpuHook, TextureViewId};

/// An opaque handle to a recorded command buffer that is ready for submission.
///
/// This ID is returned by [`CommandEncoder::finish`] and consumed by
/// [`GraphicsDevice::submit_command_buffer`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CommandBufferId(pub u64);

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
    /// This can be a performance optimization on some architectures (e.g., tile-based GPUs).
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
}

/// A descriptor for a render pass.
///
/// This struct groups all the color and depth/stencil attachments that will be used
/// in a single rendering operation.
#[derive(Debug, Default)]
pub struct RenderPassDescriptor<'a> {
    /// An optional debug label for the render pass.
    pub label: Option<&'a str>,
    /// A slice of color attachments to be used in the pass.
    pub color_attachments: &'a [RenderPassColorAttachment<'a>],
    // pub depth_stencil_attachment: Option<...>, // To be added in the future
}

/// Describes a request to write a timestamp at specific points within a pass.
///
/// This is an abstract representation that a concrete backend will translate into
/// operations on its specific query/timestamp system (e.g., `wgpu::QuerySet`).
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
