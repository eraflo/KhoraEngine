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

use crate::renderer::api::command::{CommandBufferId, ComputePassDescriptor, RenderPassDescriptor};
use crate::renderer::traits::GpuProfiler;
use crate::renderer::{BufferId, IndexFormat, RenderPipelineId};
use std::any::Any;
use std::ops::Range;

/// A trait representing an active render pass, used for recording drawing commands.
///
/// A `RenderPass` object is obtained from a [`CommandEncoder`] and provides methods
/// to set pipeline state (e.g., pipeline, vertex/index buffers) and issue draw calls.
///
/// The `'pass` lifetime ensures that the pass object cannot outlive the [`CommandEncoder`]
/// that created it, and that any resources bound to it (like buffers) also live long enough.
pub trait RenderPass<'pass> {
    /// Sets the active render pipeline for subsequent draw calls.
    fn set_pipeline(&mut self, pipeline: &'pass RenderPipelineId);

    /// Binds a vertex buffer to a specific slot.
    fn set_vertex_buffer(&mut self, slot: u32, buffer: &'pass BufferId, offset: u64);

    /// Binds an index buffer for indexed drawing.
    fn set_index_buffer(&mut self, buffer: &'pass BufferId, offset: u64, index_format: IndexFormat);

    /// Records a non-indexed draw call.
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>);

    /// Records an indexed draw call.
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>);
}

/// A trait representing an active compute pass, used for recording dispatch commands.
pub trait ComputePass<'pass> {}

/// A trait for an object that records a sequence of GPU commands.
///
/// A `CommandEncoder` is the main tool for building a [`CommandBufferId`]. It creates
/// render and compute passes, and can also record commands that happen outside of a
/// pass, such as buffer copies.
///
/// The encoder is a stateful object; its lifetime (`'encoder`) is tied to the
/// passes it creates.
pub trait CommandEncoder {
    /// Begins a new render pass, returning a mutable `RenderPass` object.
    ///
    /// The returned `RenderPass` object borrows the encoder mutably, so only one
    /// pass can be active at a time. When the `RenderPass` object is dropped,
    /// the pass is ended.
    fn begin_render_pass<'encoder>(
        &'encoder mut self,
        descriptor: &RenderPassDescriptor<'encoder>,
    ) -> Box<dyn RenderPass<'encoder> + 'encoder>;

    /// Begins a new compute pass, returning a mutable `ComputePass` object.
    fn begin_compute_pass<'encoder>(
        &'encoder mut self,
        descriptor: &ComputePassDescriptor<'encoder>,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder>;

    /// Begins a special-purpose compute pass exclusively for profiler timestamps.
    /// The implementation of this method will handle the backend-specific query details.
    fn begin_profiler_compute_pass<'encoder>(
        &'encoder mut self,
        label: Option<&str>,
        profiler: &'encoder dyn GpuProfiler,
        pass_index: u32,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder>;

    /// Records a command to copy data from one buffer to another on the GPU.
    fn copy_buffer_to_buffer(
        &mut self,
        source: &BufferId,
        source_offset: u64,
        destination: &BufferId,
        destination_offset: u64,
        size: u64,
    );

    /// Finalizes the command recording and returns a handle to the resulting command buffer.
    ///
    /// This method consumes the encoder. The returned [`CommandBufferId`] can then
    /// be submitted to the [`GraphicsDevice`]'s command queue.
    fn finish(self: Box<Self>) -> CommandBufferId;

    /// Returns a mutable reference to the underlying trait object as `Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
