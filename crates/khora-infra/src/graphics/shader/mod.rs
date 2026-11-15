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

//! The engine's WGSL sources: `lib/` modules and `pipelines/` entry points,
//! composed by [`WgpuPipelineSystem`](crate::graphics::wgpu::WgpuPipelineSystem).
//!
//! `EGUI_WGSL` is the one raw string: the egui overlay takes WGSL source, not a pipeline handle.

/// WGSL source of the egui editor overlay, a whole shader that is never composed.
pub const EGUI_WGSL: &str = include_str!("shaders/standalone/egui.wgsl");

#[cfg(test)]
mod tests;
