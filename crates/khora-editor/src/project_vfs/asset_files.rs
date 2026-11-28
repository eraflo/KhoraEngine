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

//! The project's asset files as the asset browser changes them: folders made,
//! files made, renamed, moved, duplicated and trashed, each keeping its identity.

use std::path::Path;

use anyhow::{bail, Context, Result};
use khora_sdk::khora_core::asset::{asset_key, AssetUUID};

use super::{InProject, KeyChange, ProjectVfs};

impl ProjectVfs {
    /// Creates an empty folder under `assets/` at `rel_dir` (forward-slash).
    ///
    /// The VFS indexes files only: [`Self::list_dirs`] is what shows it.
    ///
    /// # Errors
    ///
    /// As [`Self::new_on_disk`]; when the folder cannot be made.
    pub(crate) fn create_folder(&mut self, rel_dir: &str) -> Result<()> {
        let abs = self.abs_of(&self.new_on_disk(rel_dir)?);
        std::fs::create_dir_all(&abs)
            .with_context(|| format!("Failed to create folder {}", abs.display()))?;
        Ok(())
    }

    /// Renames or moves an asset or a folder from `old_rel` to `new_rel` (forward-slash, under `assets/`).
    ///
    /// **Freezes its UUID**, or that of every file in the folder, so every
    /// existing reference keeps resolving, and saves the registry once; a
    /// registry that cannot be saved moves the file or folder back. `old_rel`
    /// is named as [`Self::existing_on_disk`] names it, `new_rel` as
    /// [`Self::new_on_disk`] does. Where file names ignore case, a rename in
    /// case only renames the entry, and a folder's moves the identities its
    /// trashed files left too. Rebuilds the index. Returns the keys it moved.
    ///
    /// # Errors
    ///
    /// As [`Self::existing_on_disk`] for `old_rel` and [`Self::new_on_disk`]
    /// for `new_rel`; as [`Self::move_entry`].
    pub fn rename_asset(&mut self, old_rel: &str, new_rel: &str) -> Result<KeyChange> {
        let old_rel = self.existing_on_disk(old_rel)?;
        let new_rel = self.new_on_disk(new_rel)?;
        self.move_entry(&old_rel, &new_rel)
    }

    /// Moves an asset or a folder into `dest_dir` (forward-slash folder, `""` = assets root), keeping its name.
    ///
    /// The name it keeps is never held to a new name's rules: a file named
    /// on another system moves as it is named. Keeps identities as
    /// [`Self::rename_asset`] does, and returns the keys it moved.
    ///
    /// # Errors
    ///
    /// As [`Self::existing_on_disk`] for `src_rel` and for its path in
    /// `dest_dir`, whose new folders are held to a new name's rules; as
    /// [`Self::move_entry`].
    pub(crate) fn move_asset(&mut self, src_rel: &str, dest_dir: &str) -> Result<KeyChange> {
        let file_name = src_rel.rsplit('/').next().unwrap_or(src_rel);
        let new_rel = if dest_dir.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", dest_dir.trim_end_matches('/'), file_name)
        };
        let old_rel = self.existing_on_disk(src_rel)?;
        let new_rel = self.existing_on_disk(&new_rel)?;
        self.move_entry(&old_rel, &new_rel)
    }

    /// Sends an asset to the OS recycle bin (reversible), rebuilds the index, and returns what it trashed.
    ///
    /// The registry is left as it is: see [`Self::trash_with`].
    ///
    /// # Errors
    ///
    /// As [`Self::trash_with`].
    pub(crate) fn delete_to_trash(&mut self, rel: &str) -> Result<KeyChange> {
        self.trash_with(rel, |abs| {
            trash::delete(abs)
                .with_context(|| format!("Failed to move {} to the recycle bin", abs.display()))
        })
    }

    /// Trashes the asset or folder at `rel` through `send`, and rebuilds the index; the identity registry is not written.
    ///
    /// `send` moves what is at the absolute path it is given out of the
    /// project: to the recycle bin, for [`Self::delete_to_trash`]. Each asset
    /// trashed keeps its frozen identity in the registry, so a file restored
    /// at its path gets it back; a new file made at that path drops it first
    /// ([`Self::create_asset`], [`Self::duplicate_asset`], [`Self::write_asset`]).
    /// Returns the key trashed, with the identities of the files it held.
    ///
    /// # Errors
    ///
    /// As [`Self::existing_on_disk`]; when the identity registry was not
    /// read, before anything moves; when the asset is missing; when `send`
    /// fails (the asset left where it was).
    pub(crate) fn trash_with(
        &mut self,
        rel: &str,
        send: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<KeyChange> {
        let rel = self.existing_on_disk(rel)?;
        self.identities_kept(&format!("Moving '{rel}' to the recycle bin"))?;
        let abs = self.abs_of(&rel);
        if !abs.exists() {
            bail!("Asset does not exist: {}", abs.display());
        }
        // Listed before it goes: a folder's files are not there after.
        let files = if abs.is_dir() {
            self.files_in(&rel)
        } else {
            vec![rel.clone()]
        };
        let identities = files.iter().map(|file| self.resolve_uuid(file)).collect();
        send(&abs)?;
        self.reindex()?;
        Ok(KeyChange::Trashed {
            key: rel,
            identities,
        })
    }

    /// Duplicates an asset next to itself with a unique ` copy` suffix, returning its path.
    ///
    /// The copy is a **new asset**, with no registry entry: a fresh `new_v5`
    /// identity, the entry a trashed file left at its path dropped first
    /// (saving the registry).
    ///
    /// # Errors
    ///
    /// As [`Self::existing_on_disk`]; when the copy's name is no new name
    /// ([`super::check_new_name`]), or no free one is found; when the identity
    /// registry was not read, or cannot be saved without a trashed file's
    /// entry at the copy's path (nothing made); when the file cannot be copied.
    pub(crate) fn duplicate_asset(&mut self, rel: &str) -> Result<String> {
        let rel = self.existing_on_disk(rel)?;
        let rel = rel.as_str();
        let src_abs = self.abs_of(rel);
        if !src_abs.exists() {
            bail!("Asset does not exist: {}", src_abs.display());
        }
        let (dir, file) = match rel.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), rel.to_string()),
        };
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (file.clone(), String::new()),
        };
        // Find the first free `stem copy`, `stem copy 2`, … name.
        let mut new_rel = String::new();
        for n in 1..10_000 {
            let suffix = if n == 1 {
                " copy".to_string()
            } else {
                format!(" copy {n}")
            };
            let candidate_file = format!("{stem}{suffix}{ext}");
            super::check_new_name(&candidate_file)?;
            let candidate = if dir.is_empty() {
                candidate_file
            } else {
                format!("{dir}/{candidate_file}")
            };
            if !self.abs_of(&candidate).exists() {
                new_rel = candidate;
                break;
            }
        }
        if new_rel.is_empty() {
            bail!("Could not find a free duplicate name for {rel}");
        }
        let freed =
            self.free_path_for_new_file(&new_rel, &format!("Duplicating '{rel}' as '{new_rel}'"))?;
        let copied = std::fs::copy(&src_abs, self.abs_of(&new_rel))
            .map(drop)
            .with_context(|| format!("Failed to duplicate {}", src_abs.display()));
        self.give_back_on_failure(&new_rel, freed, copied)?;
        self.reindex()?;
        Ok(new_rel)
    }

    /// Creates a new file at `rel` (forward-slash, under `assets/`) holding `bytes`, folders too.
    ///
    /// The path is named as [`Self::new_on_disk`] names it, and an existing
    /// file is never replaced. The new file is a new asset: the entry a
    /// trashed file left at its path is dropped first, saving the registry.
    /// Rebuilds the index.
    ///
    /// # Errors
    ///
    /// As [`Self::new_on_disk`]; when a file is there; when the identity
    /// registry was not read, or cannot be saved without a trashed file's
    /// entry at the path (nothing made); when the folders or the file cannot
    /// be written.
    pub fn create_asset(&mut self, rel: &str, bytes: &[u8]) -> Result<()> {
        let rel = self.new_on_disk(rel)?;
        let rel = rel.as_str();
        let abs = self.abs_of(rel);
        let freed = self.free_path_for_new_file(rel, &format!("Making '{rel}'"))?;
        let folders = match abs.parent() {
            Some(parent) => std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display())),
            None => Ok(()),
        };
        // `create_new` refuses an existing file in the same call that opens it,
        // so nothing an author wrote is ever replaced.
        let made = folders
            .and_then(|()| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&abs)
                    .with_context(|| format!("Failed to create {}", abs.display()))
            })
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, bytes)
                    .with_context(|| format!("Failed to write {}", abs.display()))
            });
        self.give_back_on_failure(rel, freed, made)?;
        self.reindex()
    }

    /// Lists every folder under `assets/` as sorted forward-slash paths, the root excluded.
    ///
    /// The VFS indexes files only, so an empty folder shows only here. A
    /// [temporary](khora_sdk::fs::is_temporary) folder, and what it holds, is
    /// left out.
    pub(crate) fn list_dirs(&self) -> Vec<String> {
        let mut dirs = Vec::new();
        for entry in walkdir::WalkDir::new(&self.assets_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_temporary_folder(entry))
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let rel = match entry.path().strip_prefix(&self.assets_root) {
                Ok(r) if !r.as_os_str().is_empty() => r,
                _ => continue,
            };
            // Skip the engine's own `.khora` project dir if it ever sits inside.
            let rel_fwd = asset_key(rel);
            if rel_fwd.starts_with('.') {
                continue;
            }
            dirs.push(rel_fwd);
        }
        dirs.sort();
        dirs
    }

    /// Drops the identity a trashed file left at `rel` when no file is there, before a new file is made there; returns it.
    ///
    /// A trashed file keeps its frozen identity, for a restore at its path; a
    /// new file made there is another asset, which must not take it. The
    /// registry's file is followed first, and saved without the entry. With a
    /// file at `rel`, a write there is the same asset, and nothing is dropped.
    /// `what` names the operation making the file, for its refusal.
    ///
    /// # Errors
    ///
    /// When no file is at `rel` and the identity registry was not read, saying
    /// that `what` is refused: the new file could not drop the entry, and would
    /// take it once the registry reads; when the registry cannot be saved
    /// without the entry (the entry kept).
    pub(super) fn free_path_for_new_file(
        &mut self,
        rel: &str,
        what: &str,
    ) -> Result<Option<AssetUUID>> {
        if self.abs_of(rel).exists() {
            return Ok(None);
        }
        self.identities_kept(what)?;
        let uuid = self.registry.resolve(rel);
        let before = self.registry.clone();
        self.registry.remove(rel);
        if self.registry.len() == before.len() {
            return Ok(None);
        }
        if let Err(error) = self.registry.save() {
            self.registry = before;
            bail!(
                "the asset identity registry cannot be saved without the identity a trashed \
                 file left at '{rel}', so no file is made there: {error}"
            );
        }
        Ok(Some(uuid))
    }

    /// Returns `made`, the outcome of making a file at `rel`; when it failed, the identity `freed` is put back first.
    ///
    /// `freed` is what [`Self::free_path_for_new_file`] dropped: no file was
    /// made, so the trashed file keeps its identity, and the registry is saved
    /// with it again.
    ///
    /// # Errors
    ///
    /// What `made` holds, said to have lost the trashed file's identity when
    /// the registry cannot be saved with it again.
    pub(super) fn give_back_on_failure(
        &mut self,
        rel: &str,
        freed: Option<AssetUUID>,
        made: Result<()>,
    ) -> Result<()> {
        let Err(error) = made else {
            return Ok(());
        };
        let Some(uuid) = freed else {
            return Err(error);
        };
        self.registry.freeze(rel, uuid);
        match self.registry.save() {
            Ok(()) => Err(error),
            Err(again) => Err(error.context(format!(
                "the identity a trashed file left at '{rel}' is lost: the registry cannot be \
                 saved with it again: {again}"
            ))),
        }
    }

    /// Fails when the identity registry was not read, saying that `what` would break references.
    ///
    /// The registry's file is followed first: a file mended since it was read
    /// is read, one broken since (a merge's conflict markers) leaves the
    /// registry unread. An unread registry cannot keep what an operation
    /// freezes or drops, a new file's dropping of a trashed file's entry
    /// included: once it reads again, the asset's references would name
    /// another identity, or another asset.
    pub(super) fn identities_kept(&mut self, what: &str) -> Result<()> {
        self.follow_registry();
        if self.registry.is_read() {
            return Ok(());
        }
        bail!(
            "{what} is refused: the project's identity registry ({}) could not be read, so the \
             asset's references would break. Mend the file, or trust the project if it was \
             refused for its size, then try again.",
            self.registry.file_path().display()
        )
    }

    /// Returns the forward-slash path under `assets/` of the entry `rel` names, one that is there: its folders as spelled on disk, its own name as given.
    ///
    /// So an entry moved, renamed or trashed is filed under the key the index
    /// lists it by, whatever case `rel` spells its folders in where file names
    /// ignore it. Its name is held to no new name's rules.
    ///
    /// # Errors
    ///
    /// When [`super::check_existing`] refuses `rel`; as
    /// [`Self::folder_on_disk`].
    pub(super) fn existing_on_disk(&self, rel: &str) -> Result<String> {
        super::check_existing(&self.assets_root, rel)?;
        self.folder_on_disk(rel)
    }

    /// Returns the forward-slash path under `assets/` of the entry `rel` names, one to make: its folders as spelled on disk, its own name as given.
    ///
    /// As [`Self::existing_on_disk`], the entry's own name a new name, held to
    /// [`super::check_new_name`].
    ///
    /// # Errors
    ///
    /// When [`super::check_existing`] refuses `rel`, or
    /// [`super::check_new_name`] its own name; as [`Self::folder_on_disk`].
    pub(super) fn new_on_disk(&self, rel: &str) -> Result<String> {
        super::check_existing(&self.assets_root, rel)?;
        super::check_new_name(rel.rsplit('/').next().unwrap_or(rel))?;
        self.folder_on_disk(rel)
    }

    /// Moves the entry at `old_rel` to `new_rel`, both named on disk, keeping the identities it holds; rebuilds the index.
    ///
    /// Returns the keys it moved: `old_rel` to `new_rel`.
    ///
    /// # Errors
    ///
    /// When the identity registry was not read, before anything moves; when
    /// the source is missing, another entry is at the destination or the
    /// destination lies inside the folder moved, the file or folder cannot be
    /// moved, or the registry cannot be saved (moved back).
    fn move_entry(&mut self, old_rel: &str, new_rel: &str) -> Result<KeyChange> {
        let moved = KeyChange::Moved {
            from: old_rel.to_owned(),
            to: new_rel.to_owned(),
        };
        if old_rel == new_rel {
            return Ok(moved);
        }
        self.identities_kept(&format!("Moving '{old_rel}' to '{new_rel}'"))?;
        let old_abs = self.abs_of(old_rel);
        let new_abs = self.abs_of(new_rel);
        if !old_abs.exists() {
            bail!("Source asset does not exist: {}", old_abs.display());
        }
        // Where names ignore case, `Tex` renamed `tex` is the entry itself.
        let in_case_only = khora_sdk::fs::is_same_entry(&old_abs, &new_abs);
        if new_abs.exists() && !in_case_only {
            bail!("Destination already exists: {}", new_abs.display());
        }
        let into_itself = old_abs.is_dir()
            && !in_case_only
            && std::fs::canonicalize(&old_abs)
                .is_ok_and(|real| khora_sdk::fs::resolved_relative_to(&new_abs, &real).is_some());
        if into_itself {
            bail!("'{old_rel}' cannot move into itself, to '{new_rel}'");
        }
        // Listed before the move: the identities they hold move with them.
        let in_folder = old_abs.is_dir().then(|| self.files_in(old_rel));
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        khora_sdk::fs::rename_entry(&old_abs, &new_abs).with_context(|| {
            format!(
                "Failed to move {} → {}",
                old_abs.display(),
                new_abs.display()
            )
        })?;

        // Freeze identity across the move, then persist. In-memory update must
        // precede `rebuild_index` so the new path resolves to the frozen UUID.
        let before = self.registry.clone();
        // Both spellings of a folder renamed in case only are one place: a
        // file trashed from it is restored under the new one.
        match &in_folder {
            Some(files) if in_case_only => {
                (self.registry).rename_folder_in_case(
                    old_rel,
                    new_rel,
                    files.iter().map(String::as_str),
                );
            }
            Some(files) => {
                self.registry
                    .rename_folder(old_rel, new_rel, files.iter().map(String::as_str));
            }
            None => self.registry.rename(old_rel, new_rel),
        }
        if let Err(error) = self.registry.save() {
            self.registry = before;
            let moved_back = khora_sdk::fs::rename_entry(&new_abs, &old_abs);
            return Err(anyhow::anyhow!(
                "the asset identity registry cannot be saved, so '{old_rel}' {}: {error}",
                match moved_back {
                    Ok(()) => "is left where it was".to_owned(),
                    Err(back) => format!("is left at '{new_rel}' ({back})"),
                }
            ));
        }
        self.reindex()?;
        Ok(moved)
    }

    /// Returns `rel`, checked, with its folders named as [`Self::in_project`] names them.
    ///
    /// # Errors
    ///
    /// When its folders lead out of `assets/` through a link, or
    /// [`Self::in_project`] refuses them: a new folder is held to a new
    /// name's rules.
    fn folder_on_disk(&self, rel: &str) -> Result<String> {
        let Some((folder, name)) = rel.rsplit_once('/') else {
            return Ok(rel.to_owned());
        };
        match self.in_project(&self.abs_of(folder)) {
            InProject::Inside(folder) => Ok(format!("{folder}/{name}")),
            InProject::Refused(why) => bail!("Not a path under assets/: {why}"),
            InProject::Outside => {
                bail!("Not a path under assets/: '{rel}' leads out of them through a link")
            }
        }
    }

    /// Returns every file in the folder `rel_dir`, at any depth, by its forward-slash path under `assets/`.
    ///
    /// Links are listed, not followed: a link moves with its folder. What a
    /// [temporary](khora_sdk::fs::is_temporary) folder holds is left out.
    fn files_in(&self, rel_dir: &str) -> Vec<String> {
        walkdir::WalkDir::new(self.abs_of(rel_dir))
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_temporary_folder(entry))
            .filter_map(|entry| entry.ok())
            .filter(|entry| !entry.file_type().is_dir())
            .filter_map(|entry| {
                let rel = entry.path().strip_prefix(&self.assets_root).ok()?;
                Some(asset_key(rel))
            })
            .collect()
    }
}

/// Returns whether a walk's `entry` is a folder named as a [temporary](khora_sdk::fs::is_temporary), one no walk of the project goes into.
fn is_temporary_folder(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir() && khora_sdk::fs::is_temporary(&entry.file_name().to_string_lossy())
}
