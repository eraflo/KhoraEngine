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

//! The depth and stencil test state of a render pipeline.

use crate::renderer::api::pipeline::{CompareFunction, StencilOperation};
use crate::renderer::api::resource::TextureFormat;

/// Describes the state for depth and stencil testing.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthStencilStateDescriptor {
    /// The format of the depth/stencil texture.
    pub format: TextureFormat,
    /// If `true`, depth values will be written to the depth buffer.
    pub depth_write_enabled: bool,
    /// The comparison function used for the depth test.
    pub depth_compare: CompareFunction,
    /// The stencil state for front-facing primitives.
    pub stencil_front: StencilFaceState,
    /// The stencil state for back-facing primitives.
    pub stencil_back: StencilFaceState,
    /// A bitmask for reading from the stencil buffer.
    pub stencil_read_mask: u32,
    /// A bitmask for writing to the stencil buffer.
    pub stencil_write_mask: u32,
    /// The depth bias_state.
    pub bias: DepthBiasState,
}

/// Describes the stencil test and operations for a single face of a primitive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StencilFaceState {
    /// The comparison function used for the stencil test.
    pub compare: CompareFunction,
    /// The operation to perform if the stencil test fails.
    pub fail_op: StencilOperation,
    /// The operation to perform if the stencil test passes but the depth test fails.
    pub depth_fail_op: StencilOperation,
    /// The operation to perform if both the stencil and depth tests pass.
    pub depth_pass_op: StencilOperation,
}

/// Describes depth biasing, used to prevent z-fighting.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DepthBiasState {
    /// A constant value added to the depth of each fragment.
    pub constant: i32,
    /// A factor that scales with the fragment's depth slope.
    pub slope_scale: f32,
    /// The maximum bias that can be applied.
    pub clamp: f32,
}
