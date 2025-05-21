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

use std::borrow::Cow;
use crate::khora_bitflags;

khora_bitflags! {
    /// Defines the intended usage of a GPU buffer.
    /// These flags can be combined.
    pub struct BufferUsage: u32 {
        /// The buffer can be mapped for reading.
        const MAP_READ = 1 << 0;
        /// The buffer can be mapped for writing.
        const MAP_WRITE = 1 << 1;
        /// The buffer can be copied from.
        const COPY_SRC = 1 << 2;
        /// The buffer can be copied to.
        const COPY_DST = 1 << 3;
        
        const VERTEX = 1 << 4;
        const INDEX = 1 << 5;
        const UNIFORM = 1 << 6;

        /// The buffer can be used as a storage buffer (read/write access from shaders).
        const STORAGE = 1 << 7;
        /// The buffer can be used as an indirect buffer (e.g., for indirect draw calls).
        const INDIRECT = 1 << 8;
        /// The buffer can be used as a query resolve buffer.
        const QUERY_RESOLVE = 1 << 9;

        // Common combinations for convenience
        const GPU_ONLY_READ = Self::VERTEX.bits() | Self::INDEX.bits() | Self::UNIFORM.bits() | Self::STORAGE.bits() | Self::INDIRECT.bits() | Self::QUERY_RESOLVE.bits() | Self::COPY_SRC.bits();
        const CPU_WRITABLE = Self::MAP_WRITE.bits() | Self::COPY_DST.bits();
    }
}

/// Descriptor for creating a new GPU buffer.
#[derive(Debug, Clone)]
pub struct BufferDescriptor<'a> {
    pub label: Option<Cow<'a, str>>,
    pub size: u64,
    pub usage: BufferUsage,
    pub mapped_at_creation: bool
}

/// Opaque handle representing a GPU buffer managed by the GraphicsDevice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);