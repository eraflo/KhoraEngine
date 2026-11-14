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

//! How often the GPU advances through a vertex buffer.

/// Defines how often the GPU advances to the next element in a vertex buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexStepMode {
    /// The GPU advances to the next element for each vertex.
    Vertex,
    /// The GPU advances to the next element only for each new instance being rendered.
    Instance,
}
