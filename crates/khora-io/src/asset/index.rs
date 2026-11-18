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
use khora_core::asset::{asset_key, AssetMetadata, AssetSource, AssetUUID};
use std::collections::HashMap;

/// The most bytes an index may hold, and its decode may allocate.
pub const MAX_INDEX_BYTES: usize = 256 << 20;

/// The in-memory asset index (`index.bin`): each asset's metadata by its UUID.
///
/// See the book: concepts/assets.md, "The pipeline".
#[derive(Debug)]
pub struct AssetIndex {
    index: HashMap<AssetUUID, AssetMetadata>,
    /// Each file's UUID by its forward-slash path; no part is listed.
    by_path: HashMap<String, AssetUUID>,
    /// How many entries the file held, two sharing a UUID counted twice.
    entries_read: usize,
}

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

        let entries_read = metadata_vec.len();
        let mut index: HashMap<AssetUUID, AssetMetadata> = HashMap::with_capacity(entries_read);
        for meta in metadata_vec {
            let (uuid, path) = (meta.uuid, meta.source_path.clone());
            if let Some(hidden) = index.insert(uuid, meta) {
                log::warn!(
                    "Two assets share the UUID {uuid}: {} hides {}",
                    path.display(),
                    hidden.source_path.display()
                );
            }
        }

        // Only the entries kept: a file hidden by another of its UUID is at no path.
        let by_path = (index.values())
            .filter(|meta| !matches!(meta.default_source(), Some(AssetSource::Sub { .. })))
            .map(|meta| (asset_key(&meta.source_path), meta.uuid))
            .collect();

        Ok(Self {
            index,
            by_path,
            entries_read,
        })
    }

    /// Returns an index of no asset.
    pub(crate) fn empty() -> Self {
        Self {
            index: HashMap::new(),
            by_path: HashMap::new(),
            entries_read: 0,
        }
    }

    /// Returns the metadata of the asset `uuid`, if indexed.
    pub fn get_metadata(&self, uuid: &AssetUUID) -> Option<&AssetMetadata> {
        self.index.get(uuid)
    }

    /// Returns the UUID of the file indexed at the forward-slash path `rel_fwd`, if any.
    ///
    /// The file's own entry, never one of its parts, which share its path. A
    /// renamed file is found under its frozen UUID, in a loose index and in a
    /// pack's; a file hidden by another entry of its UUID is at no path.
    pub fn uuid_at(&self, rel_fwd: &str) -> Option<AssetUUID> {
        self.by_path.get(rel_fwd).copied()
    }

    /// Returns an iterator over the metadata of every indexed asset.
    pub fn iter_all(&self) -> impl Iterator<Item = &AssetMetadata> {
        self.index.values()
    }

    /// Returns the total number of indexed assets.
    pub fn asset_count(&self) -> usize {
        self.index.len()
    }

    /// Returns how many entries the index file held, two sharing a UUID
    /// counted twice: what a pack's header counts.
    pub fn entries_read(&self) -> usize {
        self.entries_read
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// One index entry, its source path `source_path`.
    fn entry(source_path: &str) -> AssetMetadata {
        AssetMetadata {
            uuid: AssetUUID::new_v5("a.wgsl"),
            source_path: PathBuf::from(source_path),
            asset_type: khora_core::asset::AssetType::Shader,
            dependencies: Vec::new(),
            variants: HashMap::from([(
                AssetMetadata::DEFAULT_VARIANT.to_owned(),
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

    /// **A file is found by its path under the UUID it is indexed under.**
    ///
    /// A renamed file under its frozen UUID, never a part sharing its path; a
    /// path no file holds under none.
    #[test]
    fn a_file_is_found_by_its_path_under_the_uuid_it_is_indexed_under() {
        let frozen = AssetUUID::new_v5("models/old_hero.glb");
        let mut file = entry("models/hero.glb");
        file.uuid = frozen;
        let part = AssetMetadata {
            uuid: AssetUUID::sub_asset(frozen, "mesh0"),
            source_path: PathBuf::from("models/hero.glb"),
            asset_type: khora_core::asset::AssetType::Mesh,
            dependencies: Vec::new(),
            variants: HashMap::from([(
                AssetMetadata::DEFAULT_VARIANT.to_owned(),
                AssetSource::Sub {
                    container: frozen,
                    label: "mesh0".to_owned(),
                },
            )]),
            tags: Vec::new(),
        };

        for entries in [[file.clone(), part.clone()], [part, file]] {
            let index = AssetIndex::new(&encoded(&entries)).expect("reads");

            assert_eq!(index.uuid_at("models/hero.glb"), Some(frozen));
            assert_eq!(index.uuid_at("models/old_hero.glb"), None);
        }
    }

    /// **Two entries sharing a UUID are both counted as read, one indexed.**
    ///
    /// The later entry hides the earlier: `asset_count` is 1, `entries_read`
    /// 2 — what a pack's header counts.
    #[test]
    fn entries_sharing_a_uuid_are_counted_as_read_and_indexed_once() {
        let index =
            AssetIndex::new(&encoded(&[entry("old.wgsl"), entry("new.wgsl")])).expect("reads");

        assert_eq!(index.asset_count(), 1);
        assert_eq!(index.entries_read(), 2);
        let kept = index
            .get_metadata(&AssetUUID::new_v5("a.wgsl"))
            .expect("the UUID is indexed");
        assert_eq!(kept.source_path, PathBuf::from("new.wgsl"));
    }

    /// **A file hidden by another sharing its UUID is found at no path.**
    ///
    /// The UUID found at a path is one whose entry is the file at that path:
    /// the hidden file's UUID names the other file, so a lookup by its path
    /// would read the other file's bytes.
    #[test]
    fn a_file_hidden_by_another_sharing_its_uuid_is_found_at_no_path() {
        let index =
            AssetIndex::new(&encoded(&[entry("old.wgsl"), entry("new.wgsl")])).expect("reads");

        assert_eq!(index.uuid_at("new.wgsl"), Some(AssetUUID::new_v5("a.wgsl")));
        let found = index
            .uuid_at("old.wgsl")
            .and_then(|uuid| index.get_metadata(&uuid))
            .map(|metadata| metadata.source_path.clone());
        assert_eq!(
            found, None,
            "old.wgsl is hidden, yet a lookup reads {found:?}"
        );
    }
}
