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

//! Flags representing which shader stages can access a resource binding.

use super::enums::ShaderStage;

crate::khora_bitflags! {
    /// Flags representing which shader stages can access a resource binding.
    ///
    /// Used in bind group layouts to specify the visibility of a resource.
    /// Combine with `|`.
    ///
    /// Built with `khora_bitflags!` like every other flag set in the engine.
    /// It used to be hand-written, which cost it the `ALL_DECLARED` constant a
    /// backend bridge needs to prove it translates every flag — so this one
    /// crossed into wgpu as a raw bit cast, correct only because the two sets
    /// happen to be numbered alike.
    pub struct ShaderStageFlags: u32 {
        /// Vertex shader stage.
        const VERTEX = 1 << 0;
        /// Fragment shader stage.
        const FRAGMENT = 1 << 1;
        /// Compute shader stage.
        const COMPUTE = 1 << 2;
        /// All graphics stages (vertex + fragment).
        const VERTEX_FRAGMENT = Self::VERTEX.bits() | Self::FRAGMENT.bits();
        /// All stages.
        const ALL = Self::VERTEX.bits() | Self::FRAGMENT.bits() | Self::COMPUTE.bits();
    }
}

impl ShaderStageFlags {
    /// No shader stages. Alias of [`Self::EMPTY`], kept because "no stages"
    /// reads better than "empty" at a binding's visibility.
    pub const NONE: Self = Self::EMPTY;

    /// The flag set for a single stage.
    pub const fn from_stage(stage: ShaderStage) -> Self {
        match stage {
            ShaderStage::Vertex => Self::VERTEX,
            ShaderStage::Fragment => Self::FRAGMENT,
            ShaderStage::Compute => Self::COMPUTE,
        }
    }
}
