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

//! Virtual File System (VFS) module for fast, in-memory asset metadata access.
//!
//! This module provides the [`VirtualFileSystem`] struct, which loads and manages
//! an index of asset metadata for efficient runtime queries. It is designed to
//! support asset loading and management by offering O(1) lookups of asset metadata
//! using asset UUIDs. The VFS is typically initialized from a packed binary index
//! file and serves as the primary source of truth for asset metadata in the engine.

use crate::asset::{AssetMetadata, AssetUUID};
use bincode;
use std::collections::HashMap;

/// The runtime representation of the asset index (`index.bin`).
///
/// The Virtual File System is a service that provides fast, in-memory access
/// to the metadata of all assets available in the packed data. It is the
/// primary source of truth for the `AssetAgent` when it needs to make decisions
/// about loading assets.
#[derive(Debug)]
pub struct VirtualFileSystem {
    /// The internal index mapping asset UUIDs to their metadata.
    /// This provides O(1) average-time lookups.
    index: HashMap<AssetUUID, AssetMetadata>,
}

impl VirtualFileSystem {
    /// Creates a new `VirtualFileSystem` by loading and parsing an index file from its raw bytes.
    ///
    /// This function is the entry point for the runtime asset system. It takes the
    /// binary data from `index.bin` and builds the in-memory lookup table.
    ///
    /// # Errors
    /// Returns a `DecodeError` if the byte slice is not a valid, bincode-encoded
    /// list of `AssetMetadata`.
    pub fn new(index_bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        let config = bincode::config::standard();
        // First, decode the bytes into a flat list of metadata.
        let (metadata_vec, _): (Vec<AssetMetadata>, _) =
            bincode::serde::decode_from_slice(index_bytes, config)?;

        // Then, build the HashMap for fast lookups.
        let index = metadata_vec
            .into_iter()
            .map(|meta| (meta.uuid, meta))
            .collect();

        Ok(Self { index })
    }

    /// Retrieves the metadata for a given asset UUID.
    ///
    /// This is the primary query method used by the `AssetAgent`.
    pub fn get_metadata(&self, uuid: &AssetUUID) -> Option<&AssetMetadata> {
        self.index.get(uuid)
    }
}
