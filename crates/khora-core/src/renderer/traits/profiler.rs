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

use super::command_recorder::CommandEncoder;
use std::any::Any;

/// A trait for GPU performance profilers that use timestamp queries.
///
/// This trait defines the interface for a system that can measure the execution time
/// of GPU operations within a frame. It is designed to be stateful and operate
/// across multiple frames due to the inherent latency of reading back data from the GPU.
///
/// An implementation of this trait will typically manage a set of timestamp query sets
/// and staging buffers to transfer timing data from the GPU to the CPU without stalling
/// the rendering pipeline.
///
/// The `Any` supertrait is required to allow for downcasting to a concrete profiler
/// type within backend-specific `CommandEncoder` implementations.
pub trait GpuProfiler: Any + Send + Sync {
    /// Attempts to read the results from a frame that finished rendering a few frames ago.
    ///
    /// This should be called once per frame, typically at the beginning, before any new
    /// commands are recorded. It checks for completed buffer mappings from previous frames
    /// and updates the internal timing statistics.
    fn try_read_previous_frame(&mut self);

    /// Encodes the commands necessary to resolve the current frame's timestamp queries.
    ///
    /// This should be called at the end of a `RenderPass` or command buffer where
    /// profiling queries were written. It converts the raw timestamp data into a
    /// format that can be read by the CPU.
    fn resolve_and_copy(&self, encoder: &mut dyn CommandEncoder);

    /// Encodes commands to copy the resolved query data into a CPU-readable staging buffer.
    ///
    /// This is typically called at the very end of the frame's command recording.
    fn copy_to_staging(&self, encoder: &mut dyn CommandEncoder, frame_index: u64);

    /// Schedules the asynchronous mapping of a staging buffer for CPU readback.
    ///
    /// This operation is non-blocking. It tells the GPU driver that we intend to
    /// read this buffer on the CPU in a future frame, once all commands for the
    /// current frame have been executed.
    fn schedule_map_after_submit(&mut self, frame_index: u64);

    /// Returns the smoothed duration of the main rendering pass in milliseconds.
    ///
    /// This value is typically averaged over several frames to provide a stable reading.
    fn last_main_pass_ms(&self) -> f32;

    /// Returns the smoothed total duration of the frame on the GPU in milliseconds.
    ///
    /// This value is typically averaged over several frames to provide a stable reading.
    fn last_frame_total_ms(&self) -> f32;

    /// Returns a reference to `self` as a `&dyn Any` trait object.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to `self` as a `&mut dyn Any` trait object.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
