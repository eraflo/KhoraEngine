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

//! The memory layout of a vertex buffer, and the attributes it holds.

use crate::renderer::api::pipeline::{VertexFormat, VertexStepMode};
use std::borrow::Cow;

/// Describes the memory layout of a single vertex buffer.
#[derive(Debug, Clone)]
pub struct VertexBufferLayoutDescriptor<'a> {
    /// The byte distance between consecutive elements in the buffer.
    pub array_stride: u64,
    /// How often the vertex buffer is advanced.
    pub step_mode: VertexStepMode,
    /// A list of attributes contained within each element of the buffer.
    pub attributes: Cow<'a, [VertexAttributeDescriptor]>,
}

/// Describes a single vertex attribute within a vertex buffer layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VertexAttributeDescriptor {
    /// The input location of this attribute in the vertex shader (e.g., `layout(location = 0)`).
    pub shader_location: u32,
    /// The format of the attribute's data.
    pub format: VertexFormat,
    /// The byte offset of this attribute from the start of the vertex.
    pub offset: u64,
}
