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

//! One frame's time on the GPU, as its timestamp queries read it back.

use std::time::Duration;

/// The time one frame kept the GPU busy, and which frame it was.
///
/// Read back a few frames after the frame ran: `frame` names it, as its
/// [`RenderStats::frame_number`](super::RenderStats::frame_number) did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuFrameTime {
    /// The frame measured, by its `RenderStats::frame_number`.
    pub frame: u64,
    /// The time between the timestamps written before and after its work.
    pub time: Duration,
}
