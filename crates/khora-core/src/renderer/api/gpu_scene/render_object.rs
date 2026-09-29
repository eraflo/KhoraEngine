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

//! Structure representing a draw call.

use crate::renderer::api::{pipeline::RenderPipelineId, resource::BufferId};

/// A low-level representation of a single draw call to be processed by a [`RenderLane`].
///
/// This structure links GPU buffers and pipelines, serving as the common data format
/// produced by ISAs (like `RenderAgent`) and consumed by specialized rendering lanes.
#[derive(Debug, Clone)]
pub struct RenderObject {
    /// The [`RenderPipelineId`] to bind for this object.
    pub pipeline: RenderPipelineId,
    /// The vertex buffer to bind.
    pub vertex_buffer: BufferId,
    /// The index buffer to bind.
    pub index_buffer: BufferId,
    /// The number of indices to draw from the index buffer.
    pub index_count: u32,
}
