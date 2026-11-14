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

//! How far a decoder may go: the bytes it reads, the memory it fills, the
//! elements and dimensions it accepts.

use super::{size_cap, FileKind, Trust};

pub use super::limit_exceeded::LimitExceeded;

/// One mebibyte.
const MIB: u64 = 1 << 20;

/// One gibibyte.
const GIB: u64 = 1 << 30;

/// What a decoder may read and produce for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limits {
    /// The most bytes the file may hold.
    pub max_bytes: u64,
    /// The most bytes its decoded value may fill.
    pub max_decoded_bytes: u64,
    /// The most elements (a mesh's decoded vertices) it may produce.
    pub max_elements: u64,
    /// The widest or tallest an image may be, in pixels.
    pub max_dimension: u32,
}

impl Limits {
    /// The limits of a file of `kind` read at `trust`.
    pub fn of(kind: FileKind, trust: Trust) -> Limits {
        let strict = trust.is_strict();
        let pick = |larger: u64, strict_value: u64| if strict { strict_value } else { larger };
        let max_bytes = size_cap(kind, trust);
        match kind {
            FileKind::Texture => Limits {
                max_bytes,
                max_decoded_bytes: pick(GIB, 256 * MIB),
                max_elements: u64::MAX,
                max_dimension: if strict { 8192 } else { 16384 },
            },
            FileKind::Gltf => Limits {
                max_bytes,
                // A glTF's decoded vertices are bounded by its elements; its
                // memory is its buffers, read as they are.
                max_decoded_bytes: pick(4 * GIB, GIB),
                max_elements: pick(1 << 24, 1 << 22),
                max_dimension: u32::MAX,
            },
            FileKind::Audio => Limits {
                max_bytes,
                max_decoded_bytes: pick(2 * GIB, 512 * MIB),
                max_elements: u64::MAX,
                max_dimension: u32::MAX,
            },
            FileKind::Font | FileKind::Text | FileKind::Other => Limits {
                max_bytes,
                max_decoded_bytes: max_bytes,
                max_elements: u64::MAX,
                max_dimension: u32::MAX,
            },
        }
    }

    /// The strictest limits: the strict row of the kind with the smallest
    /// caps — what a decoder that states none decodes within.
    pub fn strictest() -> Limits {
        Limits::of(FileKind::Text, Trust::Restricted)
    }
}
