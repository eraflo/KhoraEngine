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

//! An indexed project names each asset by its kind, the name every tool and Ergon share.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use khora_core::asset::{asset_key, AssetMetadata, AssetSource, AssetUUID};
use khora_io::asset::{AssetIndex, IndexBuilder};
use serde::Deserialize;

/// One file per kind the engine indexes, and the kind name its entry holds.
const FILES: [(&str, &str); 9] = [
    ("models/a.gltf", "mesh"),
    ("materials/a.kmat", "material"),
    ("textures/a.png", "texture"),
    ("sounds/a.ogg", "sound"),
    ("fonts/a.ttf", "font"),
    ("prefabs/a.kprefab", "prefab"),
    ("scenes/a.kscene", "scene"),
    ("shaders/a.wgsl", "shader"),
    ("scripts/a.erg", "script"),
];

/// An index entry as written, its kind read as the plain string it is written as.
///
/// The index is positional, so only the fields' order and types are read.
#[derive(Deserialize)]
struct WrittenEntry {
    _uuid: AssetUUID,
    source_path: PathBuf,
    kind: String,
    _dependencies: Vec<AssetUUID>,
    _variants: HashMap<String, AssetSource>,
    _tags: Vec<String>,
}

/// Returns the kind name each entry of `bytes` is written with, by asset key.
fn written_kinds(bytes: &[u8]) -> BTreeMap<String, String> {
    let (entries, _): (Vec<WrittenEntry>, _) =
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .expect("an index reads as entries of kind names");
    entries
        .into_iter()
        .map(|entry| (asset_key(&entry.source_path), entry.kind))
        .collect()
}

/// Writes one file of each kind under `root`.
fn write_project(root: &Path) {
    for (path, _) in FILES {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("a file has a folder")).expect("a folder");
        std::fs::write(at, b"").expect("a file is written");
    }
}

/// **An indexed project writes each asset's kind name**, a sound as `sound`,
/// never `audio`, and an index read back holds the same names.
#[test]
fn the_index_names_each_asset_by_its_kind() {
    let dir = tempfile::tempdir().expect("a temp dir");
    write_project(dir.path());

    let bytes = IndexBuilder::new(khora_io::trust::FileGuard::new(
        dir.path(),
        khora_io::trust::Trust::Project,
        khora_io::trust::Refusals::detached(),
    ))
    .build_index_bytes()
    .expect("the project is indexed");
    let expected: BTreeMap<String, String> = FILES
        .iter()
        .map(|(path, kind)| ((*path).to_owned(), (*kind).to_owned()))
        .collect();
    assert_eq!(written_kinds(&bytes), expected);
    assert!(
        !bytes.windows(b"audio".len()).any(|at| at == b"audio"),
        "no entry is written as `audio`"
    );

    let index = AssetIndex::new(&bytes).expect("the index reads back");
    assert_eq!(index.asset_count(), FILES.len());
    let mut read: Vec<AssetMetadata> = index.iter_all().cloned().collect();
    read.sort_by_key(|entry| asset_key(&entry.source_path));
    let written_again = bincode::serde::encode_to_vec(&read, bincode::config::standard())
        .expect("the index read back is written again");
    assert_eq!(written_kinds(&written_again), expected);
}
