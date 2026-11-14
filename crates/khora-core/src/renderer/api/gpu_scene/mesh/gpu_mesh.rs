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

//! The GPU-ready form of a mesh: its buffers and draw parameters.

use crate::asset::Asset;
use crate::renderer::api::pipeline::PrimitiveTopology;
use crate::renderer::api::resource::{BufferId, IndexFormat};

/// A GPU-ready representation of a mesh, containing buffer IDs and draw parameters.
pub struct GpuMesh {
    /// The vertex buffer ID containing interleaved vertex data.
    pub vertex_buffer: BufferId,
    /// The index buffer ID, if the mesh uses indexed drawing.
    pub index_buffer: BufferId,
    /// The number of vertices or indices to draw.
    pub index_count: u32,
    /// The format of indices in the index buffer.
    pub index_format: IndexFormat,
    /// The topology of primitives to render (e.g., TriangleList, TriangleStrip).
    pub primitive_topology: PrimitiveTopology,
}

impl Asset for GpuMesh {}
