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

//! The asset identity registry: the UUID frozen for each renamed asset path.
//!
//! See the book: reference/formats.md, "Asset identity registry".

use khora_core::asset::AssetUUID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Project-root subdirectory that holds engine-authored project metadata.
pub const REGISTRY_DIR: &str = ".khora";
/// File name of the asset-identity registry inside [`REGISTRY_DIR`].
pub const REGISTRY_FILE: &str = "asset-registry.ron";

/// One persisted `path → uuid` binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryEntry {
    /// Frozen identity of the asset (serialized as a hyphenated UUID string).
    uuid: AssetUUID,
    /// Forward-slash relative path under `assets/`.
    path: String,
}

/// On-disk shape of `asset-registry.ron`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RegistryFile {
    /// Bindings, sorted by UUID for deterministic, merge-friendly output.
    entries: Vec<RegistryEntry>,
}

/// A map of forward-slash relative asset paths to their frozen [`AssetUUID`].
///
/// The engine only reads it; the editor alone writes it (the methods marked
/// authoring-only).
#[derive(Debug, Clone)]
pub struct AssetIdRegistry {
    /// Project root (the directory that contains `assets/` and `.khora/`).
    project_root: PathBuf,
    /// `rel_path → frozen uuid`. Absent paths fall back to `new_v5(rel)`.
    by_path: HashMap<String, AssetUUID>,
}

impl AssetIdRegistry {
    /// Loads the registry for `project_root`.
    ///
    /// A missing or unreadable file gives an empty registry, every path on its
    /// `new_v5` default. A malformed file is logged and read as empty, so the
    /// project still opens.
    pub fn load(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        let path = project_root.join(REGISTRY_DIR).join(REGISTRY_FILE);
        let by_path = match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<RegistryFile>(&text) {
                Ok(file) => file.entries.into_iter().map(|e| (e.path, e.uuid)).collect(),
                Err(e) => {
                    log::warn!(
                        "Asset registry at {} is malformed ({e}); treating as empty.",
                        path.display()
                    );
                    HashMap::new()
                }
            },
            Err(_) => HashMap::new(),
        };
        Self {
            project_root,
            by_path,
        }
    }

    /// Returns the UUID of a forward-slash relative path: frozen, or its `new_v5` default.
    ///
    /// The index builder, the runtime and the pack builder all resolve through it,
    /// so an asset has one UUID in development and in a pack.
    pub fn resolve(&self, rel_fwd: &str) -> AssetUUID {
        self.by_path
            .get(rel_fwd)
            .copied()
            .unwrap_or_else(|| AssetUUID::new_v5(rel_fwd))
    }

    /// Number of frozen entries (assets whose path has diverged from identity).
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    /// `true` when no asset has been frozen yet.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Explicitly freezes `rel_fwd`'s identity to `uuid`.
    ///
    /// Authoring-only. Rarely needed directly — [`Self::rename`] freezes as part
    /// of the move.
    pub fn freeze(&mut self, rel_fwd: &str, uuid: AssetUUID) {
        self.by_path.insert(rel_fwd.to_string(), uuid);
    }

    /// Moves the identity of `old_rel` to `new_rel`, freezing it in the process.
    ///
    /// The new path resolves to **the same UUID** the old path resolved to
    /// (whether it was already frozen or still on its `new_v5` default), so every
    /// existing reference keeps pointing at the asset. Authoring-only.
    pub fn rename(&mut self, old_rel: &str, new_rel: &str) {
        let uuid = self.resolve(old_rel);
        self.by_path.remove(old_rel);
        self.by_path.insert(new_rel.to_string(), uuid);
    }

    /// Drops any frozen entry for `rel_fwd`, once its asset is deleted. Authoring-only.
    pub fn remove(&mut self, rel_fwd: &str) {
        self.by_path.remove(rel_fwd);
    }

    /// Absolute path of the on-disk registry file
    /// (`<project_root>/.khora/asset-registry.ron`).
    pub fn file_path(&self) -> PathBuf {
        self.project_root.join(REGISTRY_DIR).join(REGISTRY_FILE)
    }

    /// Writes the registry file if it does not exist yet. Authoring-only.
    ///
    /// A project then shows its `.khora/asset-registry.ron` from its first open.
    pub fn ensure_file(&self) -> std::io::Result<()> {
        if self.file_path().exists() {
            return Ok(());
        }
        self.save()
    }

    /// Persists the registry to `<project_root>/.khora/asset-registry.ron`.
    ///
    /// Entries are sorted by UUID, and the write is atomic: a crash mid-write
    /// leaves the previous file whole. Authoring-only.
    pub fn save(&self) -> std::io::Result<()> {
        let dir = self.project_root.join(REGISTRY_DIR);
        std::fs::create_dir_all(&dir)?;

        let mut entries: Vec<RegistryEntry> = self
            .by_path
            .iter()
            .map(|(path, uuid)| RegistryEntry {
                uuid: *uuid,
                path: path.clone(),
            })
            .collect();
        // Stable order: primary by UUID (immutable → minimal diffs on rename),
        // secondary by path to fully determine ties.
        entries.sort_by(|a, b| a.uuid.cmp(&b.uuid).then_with(|| a.path.cmp(&b.path)));

        let file = RegistryFile { entries };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let final_path = dir.join(REGISTRY_FILE);
        let tmp_path = dir.join(format!("{REGISTRY_FILE}.tmp"));
        std::fs::write(&tmp_path, text.as_bytes())?;
        std::fs::rename(&tmp_path, &final_path)?;
        log::debug!(
            "Asset registry saved ({} frozen entries) → {}",
            self.by_path.len(),
            final_path.display()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_falls_back_to_new_v5_without_entry() {
        let dir = tempdir().unwrap();
        let reg = AssetIdRegistry::load(dir.path());
        assert!(reg.is_empty());
        assert_eq!(
            reg.resolve("textures/wood.png"),
            AssetUUID::new_v5("textures/wood.png"),
            "an unfrozen path must keep the legacy path-derived UUID"
        );
    }

    #[test]
    fn rename_freezes_and_preserves_uuid() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());

        let original = reg.resolve("meshes/hero.gltf");
        reg.rename("meshes/hero.gltf", "meshes/protagonist.gltf");

        // The new path resolves to the SAME uuid the old path had — every
        // scene reference (which stored that uuid) keeps resolving.
        assert_eq!(reg.resolve("meshes/protagonist.gltf"), original);
        // The old path resolves to its own default, not to the frozen id.
        assert_eq!(
            reg.resolve("meshes/hero.gltf"),
            AssetUUID::new_v5("meshes/hero.gltf")
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("a/one.png", "a/uno.png");
        reg.rename("b/two.glb", "b/dos.glb");
        let frozen_one = reg.resolve("a/uno.png");
        let frozen_two = reg.resolve("b/dos.glb");
        reg.save().unwrap();

        let reloaded = AssetIdRegistry::load(dir.path());
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded.resolve("a/uno.png"), frozen_one);
        assert_eq!(reloaded.resolve("b/dos.glb"), frozen_two);
    }

    #[test]
    fn remove_drops_entry() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("x/a.png", "x/b.png");
        assert_eq!(reg.len(), 1);
        reg.remove("x/b.png");
        assert!(reg.is_empty());
    }
}
