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

//! glTF files and a project on disk holding them, for the editor's tests of
//! glTF sub-assets: dropping a file or one of its parts, spawning its scene,
//! listing its parts.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::MetricsRegistry;

use crate::project_vfs::ProjectVfs;

/// A glTF whose one scene shows node `Hull`: a triangle with no material.
/// Its parts: `scene`, `scene0`, `mesh0/prim0`, `material_default`.
pub const ONE_NODE_GLTF: &str = r#"{
    "asset": { "version": "2.0" },
    "scene": 0,
    "scenes": [{ "nodes": [0] }],
    "nodes": [{ "name": "Hull", "mesh": 0, "translation": [3.0, 0.0, 0.0] }],
    "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
    "buffers": [{
        "byteLength": 36,
        "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"
    }],
    "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
    "accessors": [{
        "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
        "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
    }]
}"#;

/// The base colour of [`SHIP_GLTF`]'s `material0`, worn by `Hull`.
pub const HULL_COLOR: [f32; 4] = [0.8, 0.1, 0.1, 1.0];
/// The base colour of [`SHIP_GLTF`]'s `material1`, worn by `Mast`.
pub const MAST_COLOR: [f32; 4] = [0.1, 0.2, 0.9, 1.0];

/// A glTF whose scene `Ship` shows two nodes — `Hull` (moved to x = 3) and
/// `Mast` — each a triangle in a material of its own (`material0` red,
/// `material1` blue), and holds one texture no material uses.
/// Its parts: `scene`, `scene0`, `mesh0/prim0`, `mesh1/prim0`, `material0`,
/// `material1`, `texture0`.
pub const SHIP_GLTF: &str = r#"{
    "asset": { "version": "2.0" },
    "scene": 0,
    "scenes": [{ "name": "Ship", "nodes": [0, 1] }],
    "nodes": [
        { "name": "Hull", "mesh": 0, "translation": [3.0, 0.0, 0.0] },
        { "name": "Mast", "mesh": 1 }
    ],
    "meshes": [
        { "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] },
        { "primitives": [{ "attributes": { "POSITION": 0 }, "material": 1 }] }
    ],
    "materials": [
        { "name": "Red", "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.1, 0.1, 1.0] } },
        { "name": "Blue", "pbrMetallicRoughness": { "baseColorFactor": [0.1, 0.2, 0.9, 1.0] } }
    ],
    "images": [{
        "uri": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
    }],
    "textures": [{ "source": 0 }],
    "buffers": [{
        "byteLength": 36,
        "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"
    }],
    "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
    "accessors": [{
        "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
        "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
    }]
}"#;

/// An OBJ triangle: a mesh file with no parts.
pub const TRIANGLE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

/// A project on disk, removed when the test ends.
pub struct GltfProject {
    pub root: PathBuf,
    pub vfs: Arc<Mutex<ProjectVfs>>,
}

impl Drop for GltfProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl GltfProject {
    /// Opens a fresh project named after `name` holding `files` — each a
    /// forward-slash path under `assets/` and its contents.
    pub fn with(name: &str, files: &[(&str, &str)]) -> Self {
        let root =
            std::env::temp_dir().join(format!("khora-editor-gltf-{name}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("an old project is removed");
        }
        for (rel, contents) in files {
            let path = assets_path(&root, rel);
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).expect("the asset's folder");
            }
            std::fs::write(&path, contents).expect("the asset is written");
        }
        std::fs::create_dir_all(root.join("assets")).expect("the assets folder");
        let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .expect("the project opens");
        Self {
            root,
            vfs: Arc::new(Mutex::new(vfs)),
        }
    }

    /// The identity of the file at `rel`, as the project resolves it.
    pub fn uuid(&self, rel: &str) -> AssetUUID {
        self.vfs.lock().expect("the project").resolve_uuid(rel)
    }

    /// The identity of the part `label` of the file at `rel`.
    pub fn sub(&self, rel: &str, label: &str) -> AssetUUID {
        AssetUUID::sub_asset(self.uuid(rel), label)
    }

    /// The absolute path of the file at `rel`.
    pub fn path(&self, rel: &str) -> PathBuf {
        assets_path(&self.root, rel)
    }
}

/// The browser's entry of the file at `path`, of the index's type `kind`,
/// named by its path.
pub fn file_entry(path: &str, kind: &str) -> AssetEntry {
    AssetEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        asset_type: kind.to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::new_v5(path),
        container: None,
    }
}

/// The browser's entry of the part `label` of the file at `path`, of the
/// index's type `kind`: named by its label, found at its file's path, marked
/// with its file.
pub fn part_entry(path: &str, label: &str, kind: &str) -> AssetEntry {
    let file = AssetUUID::new_v5(path);
    AssetEntry {
        name: label.to_owned(),
        asset_type: kind.to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::sub_asset(file, label),
        container: Some(file),
    }
}

fn assets_path(root: &Path, rel: &str) -> PathBuf {
    let mut path = root.join("assets");
    for segment in rel.split('/') {
        path.push(segment);
    }
    path
}
