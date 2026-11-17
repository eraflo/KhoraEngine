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
    /// `frozen uuid → rel_paths`: the paths holding a UUID (two, after a merge
    /// of two renames of one asset).
    holders: HashMap<AssetUUID, Vec<String>>,
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
        let mut holders: HashMap<AssetUUID, Vec<String>> = HashMap::new();
        for (path, uuid) in &by_path {
            holders.entry(*uuid).or_default().push(path.clone());
        }
        Self {
            project_root,
            by_path,
            holders,
        }
    }

    /// Returns the UUID of a forward-slash relative path: frozen, or its default.
    ///
    /// The default is `new_v5(rel)`. A path whose default another path holds (a
    /// rename moved it there) gets a UUID of its own, never another path's
    /// default. The index builder, the runtime and the pack builder all resolve
    /// through it, so an asset has one UUID in development and in a pack.
    pub fn resolve(&self, rel_fwd: &str) -> AssetUUID {
        if let Some(uuid) = self.by_path.get(rel_fwd) {
            return *uuid;
        }
        let mut uuid = AssetUUID::new_v5(rel_fwd);
        let mut k = 0u32;
        while self
            .holders
            .get(&uuid)
            .is_some_and(|paths| paths.iter().any(|path| path != rel_fwd))
        {
            k += 1;
            uuid = derived_uuid(rel_fwd, k);
        }
        uuid
    }

    /// Freezes each of `rels` whose UUID is derived; returns whether any was.
    ///
    /// A derived UUID (see [`Self::resolve`]) holds only while the path holding
    /// the default does: frozen, it stays once that path is gone. Authoring-only.
    pub fn freeze_derived<'a>(&mut self, rels: impl IntoIterator<Item = &'a str>) -> bool {
        let mut froze = false;
        for rel in rels {
            if self.by_path.contains_key(rel) {
                continue;
            }
            let uuid = self.resolve(rel);
            if uuid != AssetUUID::new_v5(rel) {
                self.freeze(rel, uuid);
                froze = true;
            }
        }
        froze
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
        self.remove(rel_fwd);
        self.by_path.insert(rel_fwd.to_string(), uuid);
        self.holders
            .entry(uuid)
            .or_default()
            .push(rel_fwd.to_string());
    }

    /// Moves the identity of `old_rel` to `new_rel`, freezing it in the process.
    ///
    /// The new path resolves to **the same UUID** the old path resolved to
    /// (whether it was already frozen or still on its `new_v5` default), so every
    /// existing reference keeps pointing at the asset. Authoring-only.
    pub fn rename(&mut self, old_rel: &str, new_rel: &str) {
        let uuid = self.resolve(old_rel);
        self.remove(old_rel);
        self.freeze(new_rel, uuid);
    }

    /// Drops any frozen entry for `rel_fwd`, once its asset is deleted. Authoring-only.
    pub fn remove(&mut self, rel_fwd: &str) {
        if let Some(uuid) = self.by_path.remove(rel_fwd) {
            if let Some(paths) = self.holders.get_mut(&uuid) {
                paths.retain(|path| path != rel_fwd);
                if paths.is_empty() {
                    self.holders.remove(&uuid);
                }
            }
        }
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

        // A temporary file of this write's own: two editors saving at once
        // never write into one.
        let final_path = self.file_path();
        crate::fs::write_atomic(&final_path, text.as_bytes())?;
        log::debug!(
            "Asset registry saved ({} frozen entries) → {}",
            self.by_path.len(),
            final_path.display()
        );
        Ok(())
    }
}

/// Returns `rel`'s `k`-th derived UUID: see [`AssetIdRegistry::resolve`].
///
/// Named by `rel`, a NUL and `k`: no file name holds a NUL, so a derived UUID
/// is never another path's default.
fn derived_uuid(rel: &str, k: u32) -> AssetUUID {
    AssetUUID::new_v5(&format!("{rel}\0{k}"))
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
        // A file made at the old path gets a UUID of its own: its default is
        // the moved asset's now.
        assert_ne!(reg.resolve("meshes/hero.gltf"), original);
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

    #[test]
    fn a_path_whose_default_another_path_holds_gets_a_uuid_of_its_own() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("a.png", "b.png");
        // A new file at the old path: the rename moved its default to `b.png`.
        assert_eq!(reg.resolve("b.png"), AssetUUID::new_v5("a.png"));
        assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
    }

    #[test]
    fn a_derived_uuid_frozen_stays_once_its_holder_is_gone() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("a.png", "b.png");
        assert!(reg.freeze_derived(["a.png", "b.png"]));
        reg.remove("b.png");
        assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
    }

    #[test]
    fn a_second_collision_takes_the_next_derived_uuid() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("a.png", "b.png");
        assert!(reg.freeze_derived(["a.png"]));
        reg.rename("a.png", "c.png");
        assert_eq!(reg.resolve("c.png"), derived_uuid("a.png", 1));
        assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 2));
    }

    #[test]
    fn paths_on_their_defaults_freeze_nothing() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        assert!(!reg.freeze_derived(["x.png", "y.png"]));
        assert!(reg.is_empty());
    }

    #[test]
    fn a_rename_over_a_frozen_path_frees_the_uuid_it_held() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        let held = AssetUUID::new_v5("held");
        reg.freeze("b.png", held);
        reg.rename("a.png", "b.png");
        // `held` is no one's now: a path whose default it is takes it.
        reg.freeze("c.png", AssetUUID::new_v5("c.png"));
        assert_eq!(reg.resolve("b.png"), AssetUUID::new_v5("a.png"));
        assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
    }

    #[test]
    fn a_derived_uuid_survives_a_save_and_a_load() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        reg.rename("a.png", "b.png");
        reg.freeze_derived(["a.png"]);
        reg.save().unwrap();
        let loaded = AssetIdRegistry::load(dir.path());
        assert_eq!(loaded.resolve("a.png"), derived_uuid("a.png", 1));
        assert_eq!(loaded.resolve("b.png"), AssetUUID::new_v5("a.png"));
    }

    /// **A UUID two paths hold stays held while one of them does.**
    ///
    /// A registry file holding `b.png` and `c.png` frozen to one UUID, as a
    /// merge of two renames of `a.png` writes it: once `b.png` is removed,
    /// `c.png` still holds that UUID, so a file made at `a.png` gets another,
    /// the one a runtime reading the saved file gives it.
    #[test]
    fn a_uuid_two_paths_hold_stays_held_while_one_of_them_does() {
        let dir = tempdir().unwrap();
        let moved = AssetUUID::new_v5("a.png");
        let mut written = AssetIdRegistry::load(dir.path());
        written.freeze("b.png", moved);
        written.freeze("c.png", moved);
        written.save().unwrap();

        let mut reg = AssetIdRegistry::load(dir.path());
        reg.remove("b.png");
        reg.save().unwrap();
        let reloaded = AssetIdRegistry::load(dir.path());

        assert_eq!(reg.resolve("c.png"), moved);
        assert_ne!(
            reg.resolve("a.png"),
            moved,
            "`a.png` takes the UUID `c.png` still holds"
        );
        assert_eq!(
            reg.resolve("a.png"),
            reloaded.resolve("a.png"),
            "the editor and a runtime reading the saved file disagree"
        );
    }

    /// **A derived UUID is no other path's default.**
    ///
    /// Once `a.png` is renamed to `b.png`, a file made at `a.png` derives its
    /// UUID from `a.png`, a NUL and a count; a file literally named `a.png#1`
    /// keeps its own default, before and after the editor freezes what is
    /// derived.
    #[test]
    fn a_derived_uuid_is_no_other_paths_default() {
        let dir = tempdir().unwrap();
        let mut reg = AssetIdRegistry::load(dir.path());
        let own = reg.resolve("a.png#1");
        reg.rename("a.png", "b.png");

        assert_ne!(
            reg.resolve("a.png"),
            reg.resolve("a.png#1"),
            "two files, one UUID"
        );
        reg.freeze_derived(["a.png", "a.png#1", "b.png"]);
        assert_eq!(
            reg.resolve("a.png#1"),
            own,
            "an asset no rename touched changed its UUID"
        );
        assert_ne!(reg.resolve("a.png"), own);
    }
}
