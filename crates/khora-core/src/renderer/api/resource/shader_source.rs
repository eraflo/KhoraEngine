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

//! CPU-side shader source asset.

/// A CPU-side representation of a decoded `.wgsl` shader source, the
/// hot-reload counterpart of [`CpuTexture`](super::CpuTexture).
///
/// The decoder turns raw `.wgsl` bytes into this UTF-8 string; the
/// hot-reload pump feeds it to `PipelineSystem::set_overlay_source` so the
/// affected modules recompose and their cached pipelines rebuild in place.
#[derive(Debug, Clone)]
pub struct CpuShaderSource(pub String);

impl crate::asset::Asset for CpuShaderSource {}
