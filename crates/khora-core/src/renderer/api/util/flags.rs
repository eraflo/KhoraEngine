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

/// Flags representing which shader stages can access a resource binding.
///
/// This is used in bind group layouts to specify visibility of resources.
/// Multiple stages can be combined using bitwise operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderStageFlags {
    bits: u32,
}

impl ShaderStageFlags {
    /// No shader stages.
    pub const NONE: Self = Self { bits: 0 };
    /// Vertex shader stage.
    pub const VERTEX: Self = Self { bits: 1 << 0 };
    /// Fragment shader stage.
    pub const FRAGMENT: Self = Self { bits: 1 << 1 };
    /// Compute shader stage.
    pub const COMPUTE: Self = Self { bits: 1 << 2 };
    /// All graphics stages (vertex + fragment).
    pub const VERTEX_FRAGMENT: Self = Self {
        bits: Self::VERTEX.bits | Self::FRAGMENT.bits,
    };
    /// All stages.
    pub const ALL: Self = Self {
        bits: Self::VERTEX.bits | Self::FRAGMENT.bits | Self::COMPUTE.bits,
    };

    /// Creates a new set of shader stage flags from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Creates flags from a single shader stage.
    pub const fn from_stage(stage: ShaderStage) -> Self {
        match stage {
            ShaderStage::Vertex => Self::VERTEX,
            ShaderStage::Fragment => Self::FRAGMENT,
            ShaderStage::Compute => Self::COMPUTE,
        }
    }

    /// Returns the raw bits.
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// Combines two sets of flags.
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    /// Checks if these flags contain a specific stage.
    pub const fn contains(&self, stage: ShaderStage) -> bool {
        let stage_bits = Self::from_stage(stage).bits;
        (self.bits & stage_bits) == stage_bits
    }

    /// Checks if these flags are empty (no stages).
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

impl std::ops::BitOr for ShaderStageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for ShaderStageFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}
