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

//! GPU profiling hooks.

/// Represents a specific point in a frame's GPU execution for timestamping.
///
/// These are used by a [`GpuProfiler`] to record timestamps and measure performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuHook {
    /// Marks the absolute beginning of GPU work for a frame.
    FrameStart,
    /// Marks the beginning of the main render pass.
    MainPassBegin,
    /// Marks the end of the main render pass.
    MainPassEnd,
    /// Marks the absolute end of GPU work for a frame.
    FrameEnd,
}

impl GpuHook {
    /// An array containing all `GpuHook` variants.
    pub const ALL: [GpuHook; 4] = [
        GpuHook::FrameStart,
        GpuHook::MainPassBegin,
        GpuHook::MainPassEnd,
        GpuHook::FrameEnd,
    ];
}
