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

/// A trait for a render pass, which records drawing commands.
pub trait RenderPass<'pass> {
    fn set_pipeline(&mut self, pipeline: &'pass RenderPipelineId);
    fn set_vertex_buffer(&mut self, slot: u32, buffer: &'pass BufferId, offset: u64);
    fn set_index_buffer(&mut self, buffer: &'pass BufferId, offset: u64, index_format: IndexFormat);
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>);
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>);
}

/// A trait for a compute pass, which records dispatch commands.
pub trait ComputePass<'pass> {}

/// A trait for a command encoder, which creates passes and records commands.
pub trait CommandEncoder {
    /// Begins a new render pass.
    fn begin_render_pass<'encoder>(
        &'encoder mut self,
        descriptor: &RenderPassDescriptor<'encoder>,
    ) -> Box<dyn RenderPass<'encoder> + 'encoder>;

    /// Begins a new compute pass.
    fn begin_compute_pass<'encoder>(
        &'encoder mut self,
        descriptor: &ComputePassDescriptor<'encoder>,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder>;

    /// Begins a special-purpose compute pass exclusively for profiler timestamps.
    /// The implementation will handle the backend-specific query details.
    fn begin_profiler_compute_pass<'encoder>(
        &'encoder mut self,
        label: Option<&str>,
        profiler: &'encoder dyn GpuProfiler,
        pass_index: u32, // e.g., 0 for Pass A, 1 for Pass B
    ) -> Box<dyn ComputePass<'encoder> + 'encoder>;

    /// Copies data from one buffer to another.
    /// This is essential for GPU profiling (resolving timestamps to a readable buffer).
    fn copy_buffer_to_buffer(
        &mut self,
        source: &BufferId,
        source_offset: u64,
        destination: &BufferId,
        destination_offset: u64,
        size: u64,
    );

    /// Finalizes the command recording and returns a handle to the resulting command buffer.
    fn finish(self: Box<Self>) -> CommandBufferId;

    /// Returns a mutable reference to the underlying trait object as `Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
