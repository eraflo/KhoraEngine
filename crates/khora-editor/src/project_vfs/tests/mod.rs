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

//! The project VFS: what it reads, writes, indexes and keeps of identities.

use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;

use super::*;

mod asset_files;
mod identity;
mod open;
mod reached;
mod reindex;
mod write_slot;

/// One file per asset kind the index builder knows.
///
/// The bytes are no valid asset: the probe asks whether a decoder is
/// registered for the kind, not whether it accepts the file.
const PROBES: &[&str] = &[
    "probe.obj",
    "probe.png",
    "probe.wav",
    "probe.wgsl",
    "probe.ttf",
    "probe.kscene",
    "probe.kmat",
    "probe.erg",
    "probe.kprefab",
];

/// The asset kinds a loose-files project can decode.
///
/// The SDK's `run_default` test pins the same list for the runtime: both
/// decode the same assets of one project.
const DECODABLE: &[&str] = &[
    "font", "material", "mesh", "prefab", "script", "shader", "sound", "texture",
];

/// The asset kinds among [`PROBES`] that `svc` has a decoder for, sorted.
fn decodable_types(svc: &mut AssetService) -> Result<Vec<String>> {
    let mut decodable = Vec::new();
    for file in PROBES {
        let uuid = AssetUUID::new_v5(file);
        let type_name = svc
            .index()
            .get_metadata(&uuid)
            .ok_or_else(|| anyhow::anyhow!("{file} is not indexed"))?
            .kind
            .to_string();
        let no_decoder = match svc.load::<Mesh>(&uuid) {
            Ok(_) => false,
            Err(e) => format!("{e:#}").contains("No decoder registered"),
        };
        if !no_decoder {
            decodable.push(type_name);
        }
    }
    decodable.sort();
    Ok(decodable)
}

/// A project directory under the temp folder, with an `assets/` folder,
/// removed when the test ends — panicking or not.
struct TempProject(PathBuf);

impl TempProject {
    fn new(name: &str) -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-create-asset-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir_all(root.join("assets"))?;
        Ok(Self(root))
    }

    fn open(&self) -> Result<ProjectVfs> {
        ProjectVfs::open(self.0.clone(), Arc::new(MetricsRegistry::new()))
    }

    /// Writes `bytes` at `assets/<rel>`, folders too, as an author would with the editor closed; returns its path.
    fn put(&self, rel: &str, bytes: &[u8]) -> Result<PathBuf> {
        let file = rel
            .split('/')
            .fold(self.0.join("assets"), |path, segment| path.join(segment));
        std::fs::create_dir_all(file.parent().expect("a folder"))?;
        std::fs::write(&file, bytes)?;
        Ok(file)
    }

    /// Makes the identity registry's file one byte over a trusted project's cap, so it is refused; returns its path.
    fn refuse_registry(&self) -> Result<PathBuf> {
        let registry = AssetIdRegistry::unread(&self.0).file_path();
        std::fs::create_dir_all(registry.parent().expect("a folder"))?;
        std::fs::File::create(&registry)?.set_len((64 << 20) + 1)?;
        Ok(registry)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Asserts that `outcome`, of the operation `what`, is the refusal of an unread identity registry.
fn refused_for_the_unread_registry(what: &str, outcome: Result<impl std::fmt::Debug>) {
    let error = format!("{:#}", outcome.expect_err(what));
    assert!(error.contains("is refused"), "{what}: {error}");
    assert!(error.contains("identity registry"), "{what}: {error}");
    assert!(error.contains("references would break"), "{what}: {error}");
}

/// Returns the registry saved in the project at `root`, read as a trusted project's.
fn saved_registry(root: &Path) -> AssetIdRegistry {
    AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
        root,
        khora_sdk::trust::Trust::Project,
        khora_sdk::trust::Refusals::detached(),
    ))
}

/// A glTF whose one scene shows node `Hull`: a triangle in the default
/// material.
const ONE_NODE_GLTF: &str = r#"{
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

fn reveal(rel: &str) -> AfterIndex {
    AfterIndex::Reveal(rel.to_owned())
}

/// Returns whether the file system of `project` ignores case in file names.
fn ignores_case(project: &TempProject) -> bool {
    let assets = project.0.join("assets");
    let other_case = PathBuf::from(assets.to_string_lossy().to_uppercase());
    other_case != assets && other_case.exists()
}

/// Makes `link` a link to the folder `target`: a symbolic link, or on Windows
/// a junction when a symbolic link needs a privilege the test lacks.
///
/// False when the system makes neither, and the test skips.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success())
            && link.exists()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}
