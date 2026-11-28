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
use std::collections::HashMap;
use std::path::PathBuf;

use crate::trust::{FileGuard, ProjectFile, ReadError, SecurityReason};

mod registry_file;
mod stamp;

use registry_file::{RegistryEntry, RegistryFile};
use stamp::Stamp;

/// Project-root subdirectory that holds engine-authored project metadata.
pub const REGISTRY_DIR: &str = ".khora";
/// File name of the asset-identity registry inside [`REGISTRY_DIR`].
pub const REGISTRY_FILE: &str = "asset-registry.ron";

/// A map of forward-slash relative asset paths to their frozen [`AssetUUID`].
///
/// Only an asset has an identity: a path of no asset kind (a README, a glTF's
/// `.bin`) is never frozen, and one in the file is not read. The engine only
/// reads it; the editor alone writes it (the methods marked authoring-only).
///
/// The file on disk is the truth. The registry remembers the content hash of
/// what it last read or wrote: [`Self::follow`] reads the file again when it
/// changed, and [`Self::save`] writes only over the file it read.
#[derive(Debug, Clone)]
pub struct AssetIdRegistry {
    /// Project root (the directory that contains `assets/` and `.khora/`).
    project_root: PathBuf,
    /// `rel_path → frozen uuid`. Absent paths fall back to `new_v5(rel)`.
    by_path: HashMap<String, AssetUUID>,
    /// `frozen uuid → rel_paths`: the paths holding a UUID (two, after a merge
    /// of two renames of one asset).
    holders: HashMap<AssetUUID, Vec<String>>,
    /// Whether the file was read, or is missing: a registry whose file was
    /// refused or unreadable never writes over it.
    read: bool,
    /// What the file is read through; `None` for a registry made
    /// [unread](Self::unread), which never reads it.
    guard: Option<FileGuard>,
    /// What the file held when the registry last read or wrote it.
    stamp: Stamp,
}

impl AssetIdRegistry {
    /// Loads the registry of the project whose root `project` reads under, through it.
    ///
    /// The file is read under the registry's own cap at the guard's trust; a
    /// refusal is reported to the guard's refusals. A missing file gives an
    /// empty registry, every path on its `new_v5` default. A file that does not
    /// parse (a merge's conflict markers, say) is refused for what it holds:
    /// reported to the guard's refusals, quarantined until it changes, and the
    /// registry returned [unread](Self::unread), so the project still opens and
    /// the file is never written over. The registry keeps a copy of `project`
    /// to read the file again ([`Self::follow`]) and check it before a save.
    ///
    /// # Errors
    ///
    /// [`ReadError::Refused`] when the guard refuses the file (a file refused
    /// before for what it holds, unchanged since, included), [`ReadError::Io`]
    /// when it exists but cannot be read.
    pub fn load(project: &FileGuard) -> Result<Self, ReadError> {
        let mut registry = Self::unread_under(project);
        let bytes = read_file(project)?;
        registry.take(project, bytes);
        Ok(registry)
    }

    /// Loads the registry as [`Self::load`] does; when that fails, logs why and returns it unread.
    ///
    /// Unread, it still follows its file through `project`: once the file
    /// reads, [`Self::follow`] reads it.
    pub fn load_or_unread(project: &FileGuard) -> Self {
        Self::load(project).unwrap_or_else(|error| {
            log::warn!(
                "The asset identity registry of {} is not read ({error}): every asset keeps \
                 its path's identity, and the file is not written over",
                project.root().display()
            );
            Self::unread_under(project)
        })
    }

    /// Returns an empty registry of the project at `project_root`, whose file was not read.
    ///
    /// Every path is on its `new_v5` default, [`Self::follow`] never reads the
    /// file, and [`Self::save`] never writes over it: what it holds would be
    /// lost.
    pub fn unread(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            by_path: HashMap::new(),
            holders: HashMap::new(),
            read: false,
            guard: None,
            stamp: Stamp::Unknown,
        }
    }

    /// Returns whether the registry's file was read, or is missing: `false` for
    /// one refused, unreadable or that does not parse.
    ///
    /// An unread registry gives every path its default and never writes its
    /// file: an operation that would freeze or drop an identity cannot keep it.
    pub fn is_read(&self) -> bool {
        self.read
    }

    /// Reads the registry's file again when it changed since it was last read or written; returns whether the registry changed.
    ///
    /// The registry changed when it read the file anew or became unread.
    /// Call it before an operation that freezes, drops or saves an identity,
    /// and when a reindex lands: the file on disk is the truth, and a pull or
    /// a merge may have changed it. The file is compared by the hash of its
    /// content. Changed and parsing, it is read and the registry holds what it
    /// holds; changed and not parsing, it is refused for what it holds, as
    /// [`Self::load`] refuses it, and the registry is unread; refused or
    /// unreadable, the registry is unread too. Unchanged, nothing changes. A
    /// registry made [unread](Self::unread) has no file to follow.
    pub fn follow(&mut self) -> bool {
        let Some(guard) = self.guard.take() else {
            return false;
        };
        let changed = match read_file(&guard) {
            Ok(bytes) => {
                let stamp = Stamp::of(bytes.as_deref());
                let unchanged = stamp == self.stamp;
                if !unchanged {
                    self.take(&guard, bytes);
                }
                !unchanged
            }
            Err(error) => {
                let was_read = self.read;
                if was_read {
                    log::warn!(
                        "The asset identity registry {} can no longer be read ({error}): what \
                         would change an identity is refused until it reads",
                        self.file_path().display()
                    );
                }
                self.forget(Stamp::Unknown);
                was_read
            }
        };
        self.guard = Some(guard);
        changed
    }

    /// Returns the UUID of a forward-slash relative path: frozen, or its default.
    ///
    /// The default is `new_v5(rel)`. A path whose default another path holds (a
    /// rename moved it there) gets a UUID of its own, never another path's
    /// default. The index builder, the runtime and the pack builder all resolve
    /// through it, so an asset has one UUID in development and in a pack.
    pub fn resolve(&self, rel_fwd: &str) -> AssetUUID {
        match self.by_path.get(rel_fwd) {
            Some(uuid) => *uuid,
            None => self.resolve_unfrozen(rel_fwd),
        }
    }

    /// Returns the UUID of a forward-slash relative path as if it froze none: what a new file made there gets.
    ///
    /// A file sent to the recycle bin keeps its frozen identity, so one
    /// restored at its path gets it back; a new file made at that path is
    /// another asset, whose UUID is this one once the entry is
    /// [removed](Self::remove). The path's default, or a derived UUID when
    /// another path holds that default, as [`Self::resolve`] gives it.
    pub fn resolve_unfrozen(&self, rel_fwd: &str) -> AssetUUID {
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

    /// Returns the number of frozen entries (assets whose path has diverged from identity).
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    /// Returns `true` when no asset has been frozen yet.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Freezes `rel_fwd`'s identity to `uuid`; a path of no asset kind is left unfrozen.
    ///
    /// Authoring-only. Rarely needed directly — [`Self::rename`] freezes as part
    /// of the move.
    pub fn freeze(&mut self, rel_fwd: &str, uuid: AssetUUID) {
        self.remove(rel_fwd);
        if !is_asset_path(rel_fwd) {
            return;
        }
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
    /// existing reference keeps pointing at the asset. A file moved to a path of
    /// no asset kind keeps no identity. Authoring-only.
    pub fn rename(&mut self, old_rel: &str, new_rel: &str) {
        let uuid = self.resolve(old_rel);
        self.remove(old_rel);
        self.freeze(new_rel, uuid);
    }

    /// Moves the identity of each of `files` under the folder `old_dir` to its path under `new_dir`, freezing it.
    ///
    /// `files` are the forward-slash paths of the files the folder holds: each
    /// keeps the UUID it resolved to before the move, frozen or on its default,
    /// as [`Self::rename`] keeps a file's. A path outside `old_dir` (`tex/a.png`
    /// for `textures`) is left as it is, and so is the entry of a file not
    /// listed, one in the recycle bin, which a restore at its path gets back.
    /// Authoring-only.
    pub fn rename_folder<'a>(
        &mut self,
        old_dir: &str,
        new_dir: &str,
        files: impl IntoIterator<Item = &'a str>,
    ) {
        let files: Vec<String> = files.into_iter().map(str::to_owned).collect();
        self.move_under(old_dir, new_dir, files);
    }

    /// Moves the identity of every path under `old_dir` to `new_dir`, the same folder spelled in another case, trashed files' included.
    ///
    /// Where file names ignore case, both spellings name one folder: a file
    /// trashed from it is restored under the new spelling, and gets its
    /// identity back there. `files` are those the folder holds, as for
    /// [`Self::rename_folder`]; every entry frozen under `old_dir` moves too.
    /// Authoring-only.
    pub fn rename_folder_in_case<'a>(
        &mut self,
        old_dir: &str,
        new_dir: &str,
        files: impl IntoIterator<Item = &'a str>,
    ) {
        let prefix = format!("{}/", old_dir.trim_end_matches('/'));
        let mut paths: Vec<String> = files.into_iter().map(str::to_owned).collect();
        paths.extend(
            (self.by_path.keys())
                .filter(|rel| rel.starts_with(&prefix))
                .cloned(),
        );
        paths.sort();
        paths.dedup();
        self.move_under(old_dir, new_dir, paths);
    }

    /// Drops any frozen entry for `rel_fwd`, before a new file, another asset, is made there. Authoring-only.
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

    /// Returns the absolute path of the on-disk registry file
    /// (`<project_root>/.khora/asset-registry.ron`).
    pub fn file_path(&self) -> PathBuf {
        self.project_root.join(REGISTRY_DIR).join(REGISTRY_FILE)
    }

    /// Writes the registry file if it does not exist yet. Authoring-only.
    ///
    /// A project then shows its `.khora/asset-registry.ron` from its first open.
    ///
    /// # Errors
    ///
    /// As [`Self::save`].
    pub fn ensure_file(&mut self) -> std::io::Result<()> {
        if self.file_path().exists() {
            return Ok(());
        }
        self.save()
    }

    /// Persists the registry to `<project_root>/.khora/asset-registry.ron`, only over the file it read.
    ///
    /// A compare-and-swap: the file is written only while it still holds what
    /// the registry last read or wrote (by the hash of its content), so a save
    /// never writes over a change made outside the registry — a pull, a merge,
    /// another editor. [`Self::follow`] first to take such a change in. Entries
    /// are sorted by UUID, and the write is atomic: a crash mid-write leaves
    /// the previous file whole. The registry then holds the stamp of what it
    /// wrote. Authoring-only.
    ///
    /// One writer at a time: the read, the check and the write are made under
    /// an exclusive lock on `asset-registry.ron.lock` beside the file (a
    /// [`PathLock`](crate::fs::PathLock), waited for: a save is short), so two
    /// editors saving at once never write over what the other saved. A program
    /// that takes no lock (`git`) is not kept out: the check refuses what it
    /// changed before the file is read, and a change it lands between the
    /// check and the write is written over.
    ///
    /// # Errors
    ///
    /// When the registry's file was not read ([`Self::unread`]), so writing it
    /// would lose what it holds; when the lock cannot be taken; when the file
    /// changed since it was read ([`std::io::ErrorKind::Other`]), or cannot be
    /// read to check it; when the registry cannot be serialised or the file
    /// cannot be written.
    pub fn save(&mut self) -> std::io::Result<()> {
        let guard = match &self.guard {
            Some(guard) if self.read => guard,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "the asset identity registry {} was not read, so it is not written over",
                        self.file_path().display()
                    ),
                ))
            }
        };
        let final_path = self.file_path();
        // Held to the end of the write: another editor's save reads the file
        // once this one has written it, never between its check and its write.
        let _held = crate::fs::PathLock::hold(&final_path)?;
        let on_disk = read_file(guard).map_err(|error| {
            std::io::Error::other(format!(
                "the asset identity registry {} cannot be read to check it before it is \
                 written ({error}), so it is not written over",
                self.file_path().display()
            ))
        })?;
        if Stamp::of(on_disk.as_deref()) != self.stamp {
            return Err(std::io::Error::other(format!(
                "the asset identity registry {} changed since it was read, so it is not \
                 written over",
                self.file_path().display()
            )));
        }
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

        crate::fs::write_atomic(&final_path, text.as_bytes())?;
        self.stamp = Stamp::of(Some(text.as_bytes()));
        log::debug!(
            "Asset registry saved ({} frozen entries) → {}",
            self.by_path.len(),
            final_path.display()
        );
        Ok(())
    }

    /// Returns an empty registry, not read, of the project `project` reads under, following its file through it.
    fn unread_under(project: &FileGuard) -> Self {
        Self {
            guard: Some(project.clone()),
            ..Self::unread(project.root())
        }
    }

    /// Takes in the file read as `bytes` (`None` for a missing one) through `project`.
    ///
    /// A file that parses is what the registry holds now; one that does not is
    /// refused for what it holds, and the registry is unread. Either way the
    /// registry holds the file's stamp.
    fn take(&mut self, project: &FileGuard, bytes: Option<Vec<u8>>) {
        let stamp = Stamp::of(bytes.as_deref());
        let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) else {
            self.hold(HashMap::new(), stamp);
            return;
        };
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|error| error.to_string())
            .and_then(|text| ron::from_str::<RegistryFile>(text).map_err(|e| e.to_string()));
        match parsed {
            Ok(file) => self.hold(
                file.entries
                    .into_iter()
                    .filter(|e| is_asset_path(&e.path))
                    .map(|e| (e.path, e.uuid))
                    .collect(),
                stamp,
            ),
            Err(why) => {
                project.refuse_content(
                    ProjectFile::IdentityRegistry.rel_path(),
                    SecurityReason::Malformed {
                        expected: "asset identity registry".to_owned(),
                        why,
                    },
                    &bytes,
                );
                self.forget(stamp);
            }
        }
    }

    /// Holds `by_path`, read from the file of stamp `stamp`.
    fn hold(&mut self, by_path: HashMap<String, AssetUUID>, stamp: Stamp) {
        let mut holders: HashMap<AssetUUID, Vec<String>> = HashMap::new();
        for (path, uuid) in &by_path {
            holders.entry(*uuid).or_default().push(path.clone());
        }
        self.by_path = by_path;
        self.holders = holders;
        self.read = true;
        self.stamp = stamp;
    }

    /// Drops every identity held, the registry unread, its file last seen as `stamp`.
    fn forget(&mut self, stamp: Stamp) {
        self.by_path.clear();
        self.holders.clear();
        self.read = false;
        self.stamp = stamp;
    }

    /// Moves the identity of each of `paths` under the folder `old_dir` to its path under `new_dir`, freezing it.
    ///
    /// A path outside `old_dir` is left as it is.
    fn move_under(&mut self, old_dir: &str, new_dir: &str, paths: Vec<String>) {
        let old_dir = old_dir.trim_end_matches('/');
        let new_dir = new_dir.trim_end_matches('/');
        // Every UUID is resolved before any entry moves: a derived one
        // depends on which paths hold the defaults.
        let moves: Vec<(String, String, AssetUUID)> = paths
            .into_iter()
            .filter_map(|rel| {
                let rest = rel.strip_prefix(old_dir)?.strip_prefix('/')?;
                let new = match new_dir {
                    "" => rest.to_owned(),
                    dir => format!("{dir}/{rest}"),
                };
                let uuid = self.resolve(&rel);
                Some((rel, new, uuid))
            })
            .collect();
        for (old, _, _) in &moves {
            self.remove(old);
        }
        for (_, new, uuid) in moves {
            self.freeze(&new, uuid);
        }
    }
}

/// Returns the registry's file read through `project`; `None` when it is missing.
fn read_file(project: &FileGuard) -> Result<Option<Vec<u8>>, ReadError> {
    match project.read_project_file(ProjectFile::IdentityRegistry) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.is_missing() => Ok(None),
        Err(error) => Err(error),
    }
}

/// Returns `rel`'s `k`-th derived UUID: see [`AssetIdRegistry::resolve`].
///
/// Named by `rel`, a NUL and `k`: no file name holds a NUL, so a derived UUID
/// is never another path's default.
fn derived_uuid(rel: &str, k: u32) -> AssetUUID {
    AssetUUID::new_v5(&format!("{rel}\0{k}"))
}

/// Returns whether the file at the forward-slash path `rel_fwd` is an asset: its extension names a kind.
fn is_asset_path(rel_fwd: &str) -> bool {
    super::kind_of_path(std::path::Path::new(rel_fwd)).is_some()
}

#[cfg(test)]
mod tests;
