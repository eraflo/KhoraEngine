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

//! `Assets.toml` (read by `cargo xtask assets pack` from the repository root)
//! names only directories that exist, so a later move of an example's assets
//! cannot leave the packer scanning nothing.

use std::fs;
use std::path::Path;

#[derive(serde::Deserialize)]
struct AssetManifest {
    source_directories: Vec<String>,
}

#[test]
fn assets_manifest_directories_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent directory");
    let text = fs::read_to_string(root.join("Assets.toml")).expect("read Assets.toml");
    let manifest: AssetManifest = toml::from_str(&text).expect("parse Assets.toml");
    assert!(
        !manifest.source_directories.is_empty(),
        "Assets.toml lists no source directory"
    );
    let missing: Vec<&String> = manifest
        .source_directories
        .iter()
        .filter(|dir| !root.join(dir).is_dir())
        .collect();
    assert!(
        missing.is_empty(),
        "Assets.toml names directories that do not exist: {missing:?}"
    );
}
