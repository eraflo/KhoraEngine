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

//! An entry's type is encoded as its name: an asset or pack index whose types
//! are written as strings reads, and is written back byte for byte.

use std::collections::HashMap;
use std::path::PathBuf;

use khora_core::asset::{AssetMetadata, AssetSource, AssetType, AssetUUID, CompressionKind};
use khora_io::asset::AssetIndex;
use serde::Serialize;

/// An index entry with its type written as a `String`.
#[derive(Serialize)]
struct WrittenEntry {
    uuid: AssetUUID,
    source_path: PathBuf,
    asset_type_name: String,
    dependencies: Vec<AssetUUID>,
    variants: HashMap<String, AssetSource>,
    tags: Vec<String>,
}

/// One entry per kind of type name an index holds: known, a glTF buffer, a
/// file with no extension, an unknown extension, a game's own type.
const TYPES: [(&str, &str); 7] = [
    ("textures/a.png", "texture"),
    ("sounds/a.ogg", "audio"),
    ("models/a.gltf", "mesh"),
    ("models/a.bin", "bin"),
    ("notes", "blob"),
    ("docs/README.md", "md"),
    ("quests/a.note", "ktestnote"),
];

fn written(packed: bool) -> Vec<WrittenEntry> {
    TYPES
        .iter()
        .enumerate()
        .map(|(at, (path, type_name))| {
            let source = if packed {
                AssetSource::Packed {
                    offset: at as u64 * 16,
                    size: 16,
                    uncompressed_size: 16,
                    compression: CompressionKind::None,
                }
            } else {
                AssetSource::Path(PathBuf::from(path))
            };
            WrittenEntry {
                uuid: AssetUUID::new_v5(path),
                source_path: PathBuf::from(path),
                asset_type_name: (*type_name).to_owned(),
                dependencies: Vec::new(),
                variants: HashMap::from([("default".to_owned(), source)]),
                tags: vec!["level1".to_owned()],
            }
        })
        .collect()
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    bincode::serde::encode_to_vec(value, bincode::config::standard()).expect("an index encodes")
}

/// **An index written with string types reads each type by its name**, an
/// unknown one kept as named.
#[test]
fn an_index_written_with_string_types_reads_each_type_by_its_name() {
    for packed in [false, true] {
        let index =
            AssetIndex::new(&encode(&written(packed))).expect("an index of type names reads");
        assert_eq!(index.asset_count(), TYPES.len());
        for (path, type_name) in TYPES {
            let entry = index
                .get_metadata(&AssetUUID::new_v5(path))
                .unwrap_or_else(|| panic!("{path} is indexed"));
            assert_eq!(entry.asset_type.as_str(), type_name, "{path}");
            assert!(entry.default_source().is_some(), "{path} keeps its source");
        }
        let known = |path: &str| {
            index
                .get_metadata(&AssetUUID::new_v5(path))
                .map(|entry| entry.asset_type.clone())
        };
        assert_eq!(known("textures/a.png"), Some(AssetType::Texture));
        assert_eq!(known("models/a.bin"), Some(AssetType::Buffer));
        assert_eq!(known("notes"), Some(AssetType::Blob));
        assert_eq!(
            known("docs/README.md"),
            Some(AssetType::Other("md".to_owned()))
        );
    }
}

/// **An index read and written again is the same bytes**, loose or packed.
#[test]
fn an_index_read_and_written_again_is_the_same_bytes() {
    for packed in [false, true] {
        let before = written(packed);
        let bytes = encode(&before);
        let (read, _): (Vec<AssetMetadata>, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                .expect("an index of type names reads");
        assert_eq!(encode(&read), bytes, "packed: {packed}");
    }
}
