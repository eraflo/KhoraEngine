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
/// It must also implement `Any` to allow for downcasting within backend-specific
/// implementations of the CommandEncoder.
pub trait GpuProfiler: Any + Send + Sync {
    /// Tries to read the results from a previous frame's timestamp queries.
    /// This should be called once per frame before recording new commands.
    fn try_read_previous_frame(&mut self);

    /// Records commands to resolve the internal timestamp queries into a readable buffer.
    fn resolve_and_copy(&self, encoder: &mut dyn CommandEncoder);

    /// Records commands to copy the resolved data into a staging buffer for later readback.
    fn copy_to_staging(&self, encoder: &mut dyn CommandEncoder, frame_index: u64);

    /// Schedules the asynchronous mapping of a staging buffer for CPU readback.
    /// This typically introduces a few frames of latency.
    fn schedule_map_after_submit(&mut self, frame_index: u64);

    /// Returns the smoothed duration of the main rendering pass in milliseconds.
    fn last_main_pass_ms(&self) -> f32;

    /// Returns the smoothed total duration of the frame on the GPU in milliseconds.
    fn last_frame_total_ms(&self) -> f32;

    /// Returns a reference to the underlying `Any` trait object.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the underlying `Any` trait object.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
