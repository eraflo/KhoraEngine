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

//! A container's external file as a pack holds it: a path and a span of `data.pack`.

use khora_core::asset::{AssetSource, CompressionKind};
use serde::{Deserialize, Serialize};

/// A file a container reads beside itself (a glTF's `.bin`), packed: no asset, so no kind and no uuid.
///
/// A row of the pack's external-files table, which follows the asset entries
/// in `index.bin`; the container's resolver finds it by its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedFile {
    /// Its forward-slash path under the assets root, as its container names it.
    pub path: String,
    /// Byte offset of its bytes in the asset region of `data.pack`, after the header.
    pub offset: u64,
    /// Number of bytes stored at `offset`, possibly compressed.
    pub size: u64,
    /// Its size once decompressed; `size` when stored raw.
    pub uncompressed_size: u64,
    /// How its bytes are stored.
    pub compression: CompressionKind,
}

impl PackedFile {
    /// Returns where its bytes are, as the pack's loader reads them.
    pub fn source(&self) -> AssetSource {
        AssetSource::Packed {
            offset: self.offset,
            size: self.size,
            uncompressed_size: self.uncompressed_size,
            compression: self.compression,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A packed file's source is its span of the pack**, compression included.
    #[test]
    fn a_packed_files_source_is_its_span_of_the_pack() {
        let file = PackedFile {
            path: "models/a.bin".to_owned(),
            offset: 12,
            size: 5,
            uncompressed_size: 9,
            compression: CompressionKind::Lz4,
        };
        assert!(matches!(
            file.source(),
            AssetSource::Packed {
                offset: 12,
                size: 5,
                uncompressed_size: 9,
                compression: CompressionKind::Lz4
            }
        ));
    }
}
