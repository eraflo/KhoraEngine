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

use crate::math::LinearRgba;
use crate::renderer::{GpuHook, TextureViewId};

// A unique identifier for a recorded command buffer, ready for submission.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CommandBufferId(pub u64);

/// Describes what to do with an attachment at the start of a render pass.
#[derive(Clone, Debug)]
pub enum LoadOp<V> {
    /// The existing contents of the attachment will be preserved.
    Load,
    /// The attachment will be cleared to the specified value.
    Clear(V),
}

/// Describes what to do with an attachment at the end of a render pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreOp {
    /// The results of the render pass will be written to the attachment.
    Store,
    /// The results of the render pass will be discarded.
    Discard,
}

/// Describes the load and store operations for an attachment.
#[derive(Debug)]
pub struct Operations<V> {
    pub load: LoadOp<V>,
    pub store: StoreOp,
}

/// A comprehensive description of a single color attachment for a render pass.
#[derive(Debug)]
pub struct RenderPassColorAttachment<'a> {
    /// The texture view that will be rendered to.
    pub view: &'a TextureViewId,
    /// The view that will receive the resolved output if multisampling is used.
    /// This must be `None` if the `view` is not multisampled.
    pub resolve_target: Option<&'a TextureViewId>,
    /// Defines the load and store operations for this attachment.
    pub ops: Operations<LinearRgba>,
}

/// A descriptor for a render pass, containing all its attachments.
#[derive(Debug, Default)]
pub struct RenderPassDescriptor<'a> {
    pub label: Option<&'a str>,
    pub color_attachments: &'a [RenderPassColorAttachment<'a>],
    // pub depth_stencil_attachment: Option<...>, // To be added in the future
}

/// Describes a request to write a timestamp at the beginning and/or end of a pass.
/// This is an abstract representation that the backend will translate into
/// operations on its specific query/timestamp system.
#[derive(Debug, Default)]
pub struct PassTimestampWrites<'a> {
    /// The abstract event hook to record at the beginning of the pass.
    pub beginning_of_pass_hook: Option<&'a GpuHook>,
    /// The abstract event hook to record at the end of the pass.
    pub end_of_pass_hook: Option<&'a GpuHook>,
}

/// A descriptor for a compute pass.
#[derive(Debug, Default)]
pub struct ComputePassDescriptor<'a> {
    pub label: Option<&'a str>,
    /// Optional timestamp recording requests for this pass.
    pub timestamp_writes: Option<PassTimestampWrites<'a>>,
}
