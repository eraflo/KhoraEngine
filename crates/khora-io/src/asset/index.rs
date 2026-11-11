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
//! This module provides the [`AssetIndex`] struct, which loads and manages
//! an index of asset metadata for efficient runtime queries. It is designed to
//! support asset loading and management by offering O(1) lookups of asset metadata
//! using asset UUIDs. The VFS is typically initialized from a packed binary index
//! file and serves as the primary source of truth for asset metadata in the engine.

use bincode;
use khora_core::asset::{AssetMetadata, AssetUUID};
use std::collections::HashMap;

/// The runtime representation of the asset index (`index.bin`).
///
/// The Virtual File System is a service that provides fast, in-memory access
/// to the metadata of all assets available in the packed data. It is the
/// primary source of truth for the `AssetAgent` when it needs to make decisions
/// about loading assets.
#[derive(Debug)]
pub struct AssetIndex {
    /// The internal index mapping asset UUIDs to their metadata.
    /// This provides O(1) average-time lookups.
    index: HashMap<AssetUUID, AssetMetadata>,
}

/// The most bytes an index may hold, and its decode may allocate.
pub const MAX_INDEX_BYTES: usize = 256 << 20;

impl AssetIndex {
    /// Creates a new `AssetIndex` by loading and parsing an index file from its raw bytes.
    ///
    /// This function is the entry point for the runtime asset system. It takes the
    /// binary data from `index.bin` and builds the in-memory lookup table.
    ///
    /// # Errors
    /// Returns a `DecodeError` if the byte slice is not a valid, bincode-encoded
    /// list of `AssetMetadata`.
    pub fn new(index_bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        // Bounded: a length in the file never makes the decode allocate past
        // what an index may hold.
        let config = bincode::config::standard().with_limit::<{ MAX_INDEX_BYTES }>();
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

    /// An index of no asset.
    pub(crate) fn empty() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    /// Retrieves the metadata for a given asset UUID.
    ///
    /// This is the primary query method used by the `AssetAgent`.
    pub fn get_metadata(&self, uuid: &AssetUUID) -> Option<&AssetMetadata> {
        self.index.get(uuid)
    }

    /// Returns an iterator over all asset metadata entries in the VFS.
    ///
    /// Useful for editor tooling (asset browser) that needs to display all
    /// available assets.
    pub fn iter_all(&self) -> impl Iterator<Item = &AssetMetadata> {
        self.index.values()
    }

    /// Returns the total number of indexed assets.
    pub fn asset_count(&self) -> usize {
        self.index.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::AssetSource;
    use std::path::PathBuf;

    /// One index entry, its source path `source_path`.
    fn entry(source_path: &str) -> AssetMetadata {
        AssetMetadata {
            uuid: AssetUUID::new_v5("a.wgsl"),
            source_path: PathBuf::from(source_path),
            asset_type_name: "shader".to_owned(),
            dependencies: Vec::new(),
            variants: HashMap::from([(
                "default".to_owned(),
                AssetSource::Path(PathBuf::from("a.wgsl")),
            )]),
            tags: Vec::new(),
        }
    }

    fn encoded(entries: &[AssetMetadata]) -> Vec<u8> {
        bincode::serde::encode_to_vec(entries, bincode::config::standard())
            .expect("the index encodes")
    }

    /// **An index over its limit is refused**: an `index.bin` whose first
    /// entry declares a source path of 300 MiB — in a file of a few dozen
    /// bytes — is refused as over the index's byte limit
    /// (`DecodeError::LimitExceeded`), rather than 300 MiB being allocated
    /// for it before the file is found to end. A well-formed index still
    /// reads.
    #[test]
    fn an_index_over_its_limit_is_refused() {
        let empty_path = encoded(&[entry("")]);
        let one_byte_path = encoded(&[entry("x")]);
        // Where the two encodings part is the source path's length.
        let at = (empty_path.iter().zip(&one_byte_path))
            .position(|(a, b)| a != b)
            .expect("the source paths' lengths differ");
        assert_eq!(empty_path[at], 0, "an empty path is a zero length");

        let mut forged = empty_path[..at].to_vec();
        // bincode's varint: 0xFC, then a u32, little-endian.
        forged.push(0xFC);
        forged.extend_from_slice(&(300u32 << 20).to_le_bytes());

        match AssetIndex::new(&forged) {
            Err(bincode::error::DecodeError::LimitExceeded) => {}
            Err(other) => panic!("refused for its size, not: {other:?}"),
            Ok(index) => panic!("{} assets read from a forged index", index.asset_count()),
        }

        let index = AssetIndex::new(&encoded(&[entry("a.wgsl")])).expect("a real index reads");
        assert_eq!(index.asset_count(), 1);
    }
}
