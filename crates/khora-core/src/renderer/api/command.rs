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

/// A generalized draw command containing all necessary state and bindings for a single draw call.
/// Used by RenderLanes to collect and sort draw calls before recording them.
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// The render pipeline to use for this draw call.
    pub pipeline: crate::renderer::RenderPipelineId,
    /// The vertex buffer containing the geometry.
    pub vertex_buffer: crate::renderer::BufferId,
    /// The index buffer defining the draw order.
    pub index_buffer: crate::renderer::BufferId,
    /// The format of the indices (16-bit or 32-bit).
    pub index_format: crate::renderer::IndexFormat,
    /// The number of indices to draw.
    pub index_count: u32,
    /// An optional bind group for model-specific uniforms (typically group 1).
    pub model_bind_group: Option<crate::renderer::BindGroupId>,
    /// Optional dynamic offset for the model bind group.
    pub model_offset: u32,
    /// An optional bind group for material-specific uniforms (typically group 2).
    pub material_bind_group: Option<crate::renderer::BindGroupId>,
    /// Optional dynamic offset for the material bind group.
    pub material_offset: u32,
}

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

/// A comprehensive description of a depth/stencil attachment for a render pass.
///
/// This struct describes how the depth and stencil aspects of a texture should be
/// handled during a render pass. Both aspects are optional - if an aspect's operations
/// are `None`, that aspect will be loaded from the texture and its contents preserved.
#[derive(Debug)]
pub struct RenderPassDepthStencilAttachment<'a> {
    /// The [`TextureViewId`] for the depth/stencil texture.
    /// This texture must have been created with a depth/stencil format
    /// (e.g., `Depth32Float`, `Depth24PlusStencil8`).
    pub view: &'a TextureViewId,
    /// The load and store operations for the depth aspect.
    /// If `None`, the depth aspect will be loaded and stored (read-only depth test).
    pub depth_ops: Option<Operations<f32>>,
    /// The load and store operations for the stencil aspect.
    /// If `None`, the stencil aspect will be loaded and stored (read-only stencil test).
    pub stencil_ops: Option<Operations<u32>>,
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
    /// An optional depth/stencil attachment for this pass.
    /// When set, enables depth testing and/or stencil operations.
    pub depth_stencil_attachment: Option<RenderPassDepthStencilAttachment<'a>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::TextureViewId;

    #[test]
    fn test_load_op_variants() {
        // Test Clear variant for color (LinearRgba)
        let clear_color: LoadOp<LinearRgba> = LoadOp::Clear(LinearRgba::new(1.0, 0.5, 0.0, 1.0));
        assert!(matches!(clear_color, LoadOp::Clear(_)));

        // Test Load variant
        let load: LoadOp<LinearRgba> = LoadOp::Load;
        assert!(matches!(load, LoadOp::Load));

        // Test Clear variant for depth (f32)
        let clear_depth: LoadOp<f32> = LoadOp::Clear(1.0);
        assert!(matches!(clear_depth, LoadOp::Clear(v) if (v - 1.0).abs() < f32::EPSILON));

        // Test Clear variant for stencil (u32)
        let clear_stencil: LoadOp<u32> = LoadOp::Clear(0);
        assert!(matches!(clear_stencil, LoadOp::Clear(0)));
    }

    #[test]
    fn test_store_op_variants() {
        let store = StoreOp::Store;
        let discard = StoreOp::Discard;

        assert_eq!(store, StoreOp::Store);
        assert_eq!(discard, StoreOp::Discard);
        assert_ne!(store, discard);
    }

    #[test]
    fn test_depth_stencil_attachment_creation() {
        let view_id = TextureViewId(42);

        let attachment = RenderPassDepthStencilAttachment {
            view: &view_id,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        };

        assert_eq!(*attachment.view, TextureViewId(42));
        assert!(attachment.depth_ops.is_some());
        assert!(attachment.stencil_ops.is_none());
    }

    #[test]
    fn test_render_pass_descriptor_with_depth() {
        let view_id = TextureViewId(1);
        let depth_view_id = TextureViewId(2);

        let color_attachment = RenderPassColorAttachment {
            view: &view_id,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::new(0.0, 0.0, 0.0, 1.0)),
                store: StoreOp::Store,
            },
        };

        let depth_attachment = RenderPassDepthStencilAttachment {
            view: &depth_view_id,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        };

        let descriptor = RenderPassDescriptor {
            label: Some("Test Pass"),
            color_attachments: std::slice::from_ref(&color_attachment),
            depth_stencil_attachment: Some(depth_attachment),
        };

        assert_eq!(descriptor.label, Some("Test Pass"));
        assert_eq!(descriptor.color_attachments.len(), 1);
        assert!(descriptor.depth_stencil_attachment.is_some());
    }

    #[test]
    fn test_render_pass_descriptor_default() {
        let descriptor = RenderPassDescriptor::default();

        assert!(descriptor.label.is_none());
        assert!(descriptor.color_attachments.is_empty());
        assert!(descriptor.depth_stencil_attachment.is_none());
    }
}
