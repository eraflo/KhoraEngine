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

//! Defines the unified file format for Khora scenes.
//!
//! Every scene persisted by the SAA-Serialize system uses this container format.
//! It consists of a fixed-size [`SceneHeader`] followed by a variable-length payload.
//! The header acts as a manifest, describing what serialization strategy was used
//! to encode the payload, allowing the engine to correctly dispatch the data to the
//! appropriate deserialization `Lane`.

use std::convert::TryInto;

/// A unique byte sequence to identify Khora Scene Files. ("KHORASCN").
pub const HEADER_MAGIC_BYTES: [u8; 8] = *b"KHORASCN";
const STRATEGY_ID_LEN: usize = 32;

/// The fixed-size header at the beginning of every Khora scene file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneHeader {
    /// Magic bytes to identify the file type, must be `HEADER_MAGIC_BYTES`.
    pub magic_bytes: [u8; 8],
    /// The version of the header format itself.
    pub format_version: u8,
    /// A null-padded UTF-8 string identifying the serialization strategy used.
    /// e.g., "KH_RECIPE_V1", "KH_ARCHETYPE_V1".
    pub strategy_id: [u8; STRATEGY_ID_LEN],
    /// The length of the payload data that follows this header, in bytes.
    pub payload_length: u64,
}

/// A logical representation of a full scene file in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFile {
    /// The parsed header data.
    pub header: SceneHeader,
    /// The raw, variable-length payload data.
    pub payload: Vec<u8>,
}

// NOTE: We are intentionally not using `serde` for the header.
// It's a fixed-layout, performance-critical part of the file format,
// so direct byte manipulation is more robust and efficient.
impl SceneHeader {
    /// The total size of the header in bytes.
    pub const SIZE: usize = 8 + 1 + STRATEGY_ID_LEN + 8;

    /// Attempts to parse a `SceneHeader` from the beginning of a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < Self::SIZE {
            return Err("Not enough bytes to form a valid header");
        }

        let magic_bytes: [u8; 8] = bytes[0..8].try_into().unwrap();
        if magic_bytes != HEADER_MAGIC_BYTES {
            return Err("Invalid magic bytes; not a Khora scene file");
        }
        
        let format_version = bytes[8];

        let strategy_id: [u8; STRATEGY_ID_LEN] = bytes[9..9 + STRATEGY_ID_LEN]
            .try_into()
            .unwrap();

        let payload_length = u64::from_le_bytes(
            bytes[9 + STRATEGY_ID_LEN..Self::SIZE].try_into().unwrap(),
        );

        Ok(Self {
            magic_bytes,
            format_version,
            strategy_id,
            payload_length,
        })
    }
}