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

//! The asset index: every asset's metadata by UUID, the table of the VFS.

use bincode;
use khora_core::asset::{AssetMetadata, AssetUUID};
use std::collections::HashMap;

/// The in-memory asset index (`index.bin`): each asset's metadata by its UUID.
///
/// See the book: concepts/assets.md, "The pipeline".
#[derive(Debug)]
pub struct AssetIndex {
    index: HashMap<AssetUUID, AssetMetadata>,
}

/// The most bytes an index may hold, and its decode may allocate.
pub const MAX_INDEX_BYTES: usize = 256 << 20;

impl AssetIndex {
    /// Reads an index from the bytes of an `index.bin`.
    ///
    /// # Errors
    /// Returns a `DecodeError` if the bytes are not a bincode-encoded list of
    /// `AssetMetadata`, or would decode past [`MAX_INDEX_BYTES`].
    pub fn new(index_bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        // Bounded: a length in the file never makes the decode allocate past
        // what an index may hold.
        let config = bincode::config::standard().with_limit::<{ MAX_INDEX_BYTES }>();
        let (metadata_vec, _): (Vec<AssetMetadata>, _) =
            bincode::serde::decode_from_slice(index_bytes, config)?;

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

    /// Returns the metadata of the asset `uuid`, if indexed.
    pub fn get_metadata(&self, uuid: &AssetUUID) -> Option<&AssetMetadata> {
        self.index.get(uuid)
    }

    /// Returns an iterator over the metadata of every indexed asset.
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

    /// **An index over its limit is refused** before it allocates.
    ///
    /// A few dozen bytes declaring a 300 MiB source path give
    /// `DecodeError::LimitExceeded`; a well-formed index still reads.
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
