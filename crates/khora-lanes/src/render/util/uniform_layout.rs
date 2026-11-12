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

//! The bind-group layout of a group holding one uniform buffer at binding 0.

use khora_core::renderer::api::command::{BindGroupLayoutEntry, BindingType, BufferBindingType};
use khora_core::renderer::api::util::ShaderStageFlags;

/// A layout of one uniform buffer at binding 0, seen by `visibility`.
///
/// `dynamic_offset` is for a buffer shared by several draws, each bound at its
/// own offset.
pub(crate) fn single_uniform_layout(
    visibility: ShaderStageFlags,
    dynamic_offset: bool,
) -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: dynamic_offset,
            min_binding_size: None,
        },
    }]
}
