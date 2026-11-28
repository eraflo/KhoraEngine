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

//! A write to a project file reserved before it is made: the file it lands
//! on, and the asset id that file holds once written.

use std::path::Path;

use anyhow::{bail, Result};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::AssetWriter;

use super::{InProject, ProjectVfs};

/// The file a write under `assets/` lands on, and the id it holds once written.
///
/// Made by [`ProjectVfs::reserve_write`], written by [`ProjectVfs::write_reserved`]:
/// the id is known before the write, for a file that names itself (a prefab)
/// or that is assigned (a material). A slot is handed out only while the
/// identity registry is read, so its id is the one the file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteSlot {
    /// The file, forward-slash under `assets/`, its folders as spelled on disk.
    rel: String,
    /// What the file holds once written.
    id: AssetUUID,
    /// Whether no file is there: the write makes a new asset.
    new_file: bool,
}

impl WriteSlot {
    /// Returns the file the write lands on, forward-slash under `assets/`, as spelled on disk.
    pub fn rel(&self) -> &str {
        &self.rel
    }

    /// Returns the asset id the file holds once written.
    pub fn id(&self) -> AssetUUID {
        self.id
    }
}

impl ProjectVfs {
    /// Reserves a write to `rel_path` under `assets/`: the file it lands on and the id it holds once written.
    ///
    /// The registry's file is followed first. A file already there, however
    /// `rel_path` spells it where file names ignore case, is the same asset and
    /// keeps its identity; with none, the write makes a new asset, which never
    /// takes the identity a trashed file left at the path. Nothing is written.
    /// A write that needs no id is [`Self::write_asset`].
    ///
    /// # Errors
    ///
    /// When `rel_path` is refused ([`super::check_existing`]), leads out of
    /// `assets/` through a link, or makes a part that is no new name
    /// ([`super::check_new_name`]); when the identity registry was not read: the
    /// id of a file there is unknown (a rename may have frozen another), and a
    /// new file could not drop a trashed file's entry.
    pub fn reserve_write(&mut self, rel_path: &str) -> Result<WriteSlot> {
        self.follow_registry();
        let (rel, new_file) = self.landing(rel_path)?;
        self.identities_kept(&what_reserves(&rel, new_file))?;
        Ok(self.slot_at(rel, new_file))
    }

    /// Writes `bytes` to the file `slot` reserved, creating its folders; returns the id the file holds, the slot's.
    ///
    /// A new file drops the entry a trashed file left at its path first,
    /// saving the registry, and gives it back when the write fails. Not
    /// indexed: call [`Self::rebuild_index`] for the path to resolve through
    /// the VFS in the same frame.
    ///
    /// # Errors
    ///
    /// When the registry's file or the project's files changed since the
    /// reservation, so the file would hold another id (nothing written: reserve
    /// again); as [`Self::reserve_write`]; when the registry cannot be saved
    /// without a trashed file's entry at the path (nothing written); when the
    /// file cannot be written.
    pub fn write_reserved(&mut self, slot: WriteSlot, bytes: &[u8]) -> Result<AssetUUID> {
        self.follow_registry();
        self.identities_kept(&what_reserves(&slot.rel, slot.new_file))?;
        let freed = self
            .free_path_for_new_file(&slot.rel, &format!("Saving a new file at '{}'", slot.rel))?;
        let written = match self.landing(&slot.rel) {
            Ok((rel, new_file)) => match self.slot_at(rel, new_file) {
                now if now == slot => self
                    .file_loader
                    .write_bytes(Path::new(&slot.rel), bytes)
                    .map(|()| self.moved_on()),
                now => Err(anyhow::anyhow!(
                    "'{}' would hold the asset id {} rather than the {} reserved for it: the \
                     project changed since; nothing is written",
                    slot.rel,
                    now.id,
                    slot.id
                )),
            },
            Err(error) => Err(error),
        };
        self.give_back_on_failure(&slot.rel, freed, written)?;
        Ok(slot.id)
    }

    /// Returns the file a write to `rel_fwd` lands on, forward-slash as spelled on disk, and whether none is there yet.
    ///
    /// A file there is written over by its path; a new one is made at a new
    /// name, held to [`super::check_new_name`], folders made included.
    ///
    /// # Errors
    ///
    /// When `rel_fwd` is refused ([`super::check_existing`]), leads out of
    /// `assets/` through a link (nothing is ever written through such a link),
    /// or names a new part that is no new name ([`Self::in_project`]).
    pub(super) fn landing(&self, rel_fwd: &str) -> Result<(String, bool)> {
        super::check_existing(&self.assets_root, rel_fwd)?;
        let rel = match self.in_project(&self.abs_of(rel_fwd)) {
            InProject::Inside(rel) => rel,
            InProject::Refused(why) => bail!("Not a path under assets/: {why}"),
            InProject::Outside => {
                bail!("Not a path under assets/: '{rel_fwd}' leads out of them through a link")
            }
        };
        let new_file = !self.abs_of(&rel).is_file();
        Ok((rel, new_file))
    }

    /// Returns the slot of a write landing on `rel`, holding the id the file there holds after it, the registry as it is.
    pub(super) fn slot_at(&self, rel: String, new_file: bool) -> WriteSlot {
        let id = if new_file {
            self.registry.resolve_unfrozen(&rel)
        } else {
            self.registry.resolve(&rel)
        };
        WriteSlot { rel, id, new_file }
    }
}

/// Returns what a reserved write to `rel` does, named in its refusal while the registry is unread.
fn what_reserves(rel: &str, new_file: bool) -> String {
    if new_file {
        format!("Saving a new file at '{rel}'")
    } else {
        format!("Saving '{rel}' under the asset id it holds")
    }
}
