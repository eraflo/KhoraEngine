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

//! Defines data structures related to GPU buffer resources.

use crate::khora_bitflags;
use std::borrow::Cow;

khora_bitflags! {
    /// A set of flags describing the allowed usages of a [`BufferId`].
    ///
    /// These flags are crucial for performance and validation. The graphics driver uses them
    /// to place the buffer in the most optimal memory type (e.g., GPU-only vs. CPU-visible)
    /// and to validate that the buffer is used correctly at runtime.
    pub struct BufferUsage: u32 {
        /// The buffer can be mapped for reading on the CPU.
        const MAP_READ = 1 << 0;
        /// The buffer can be mapped for writing on the CPU.
        const MAP_WRITE = 1 << 1;
        /// The buffer can be used as the source of a copy operation.
        const COPY_SRC = 1 << 2;
        /// The buffer can be used as the destination of a copy operation.
        const COPY_DST = 1 << 3;

        /// The buffer can be bound as a vertex buffer.
        const VERTEX = 1 << 4;
        /// The buffer can be bound as an index buffer.
        const INDEX = 1 << 5;
        /// The buffer can be bound as a uniform buffer.
        const UNIFORM = 1 << 6;

        /// The buffer can be bound as a storage buffer (read/write access from shaders).
        const STORAGE = 1 << 7;
        /// The buffer can be used for indirect draw or dispatch commands.
        const INDIRECT = 1 << 8;
        /// The buffer can be used as a destination for query results.
        const QUERY_RESOLVE = 1 << 9;
    }
}

/// A descriptor used to create a [`BufferId`].
#[derive(Debug, Clone)]
pub struct BufferDescriptor<'a> {
    /// An optional debug label for the buffer.
    pub label: Option<Cow<'a, str>>,
    /// The total size of the buffer in bytes.
    pub size: u64,
    /// A bitmask of [`BufferUsage`] flags describing how the buffer will be used.
    pub usage: BufferUsage,
    /// If `true`, the buffer will be created in a mapped state, ready for immediate
    /// CPU access. This is useful for staging buffers that will be written to from the CPU.
    pub mapped_at_creation: bool,
}

/// An opaque handle to a GPU buffer resource.
///
/// This ID is returned by [`GraphicsDevice::create_buffer`] and is used to reference
/// the buffer in all subsequent operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);
