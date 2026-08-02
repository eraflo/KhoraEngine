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

//! File access for scripts, bounded by construction.
//!
//! A game has to save. Forbidding file access outright would make that
//! impossible; allowing raw paths would let a script write anywhere on the
//! player's machine, and a script is the part of a game most likely to come from
//! a mod, an asset pack, or someone other than the author. So neither: paths are
//! named against **virtual roots**, and a path that would leave its root is an
//! error rather than a warning.
//!
//! | Root | Access | Holds |
//! |---|---|---|
//! | `save://` | read/write | per-player saves |
//! | `config://` | read/write | preferences |
//! | `data://` | **read-only** | the project's own assets |
//!
//! ```text
//! File.Write("save://slot1.json", state)   // fine
//! File.Write("data://weapons.ron", …)      // refused — data:// is read-only
//! File.Read("save://../../etc/passwd")     // refused — leaves its root
//! ```
//!
//! # Not the [`VirtualFileSystem`]
//!
//! Despite the neighbouring name, [`crate::vfs::VirtualFileSystem`] is an
//! **index of asset metadata** — it answers "which UUID is this asset", not
//! "open this file". It does not cover this need and this does not replace it.
//!
//! # Where the roots come from
//!
//! Taken as explicit paths rather than discovered here. The platform's save and
//! config directories are the host application's decision — the editor, the
//! runtime and a test all place them differently — and a filesystem sandbox that
//! picks its own boundaries is one that cannot be tested against a temporary
//! directory.

mod path;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

/// One of the three places a script may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Root {
    /// `save://` — per-player save data. Read and write.
    Save,
    /// `config://` — preferences. Read and write.
    Config,
    /// `data://` — the project's assets. Read only, because a script rewriting
    /// the game's own content would corrupt an installation rather than a save.
    Data,
}

impl Root {
    /// The scheme that names it.
    pub fn scheme(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Config => "config",
            Self::Data => "data",
        }
    }

    /// The root a scheme names, if any.
    pub fn from_scheme(scheme: &str) -> Option<Self> {
        match scheme {
            "save" => Some(Self::Save),
            "config" => Some(Self::Config),
            "data" => Some(Self::Data),
            _ => None,
        }
    }

    /// Whether a script may write here.
    pub fn is_writable(self) -> bool {
        !matches!(self, Self::Data)
    }
}

impl std::fmt::Display for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://", self.scheme())
    }
}

/// Why a file operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// The path names no root at all.
    NoRoot(String),
    /// The path names a root that does not exist.
    UnknownRoot(String),
    /// The path is absolute, so it names a place rather than a place *inside*.
    Absolute(String),
    /// The path leaves its root — through `..`, a separator, or a symlink.
    Escapes(String),
    /// The root is read-only.
    ReadOnly(Root),
    /// There is nothing there.
    NotFound(String),
    /// The filesystem refused.
    Io(String),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRoot(path) => write!(
                f,
                "'{path}' does not start with a root — paths are written 'save://…', \
                 'config://…' or 'data://…'"
            ),
            Self::UnknownRoot(scheme) => write!(
                f,
                "'{scheme}://' is not a root — the roots are save://, config:// and data://"
            ),
            Self::Absolute(path) => write!(
                f,
                "'{path}' is absolute; a path is relative to its root, so that a game \
                 reaches the same file on every machine"
            ),
            Self::Escapes(path) => write!(
                f,
                "'{path}' leaves its root — a script may only reach what the root holds"
            ),
            Self::ReadOnly(root) => write!(
                f,
                "{root} is read-only — the project's own assets are not a place to save to; \
                 use save:// or config://"
            ),
            Self::NotFound(path) => write!(f, "'{path}' does not exist"),
            Self::Io(reason) => write!(f, "the filesystem refused: {reason}"),
        }
    }
}

impl std::error::Error for FsError {}

/// The three roots a script may reach, and nothing else.
#[derive(Debug, Clone)]
pub struct SandboxFs {
    save: PathBuf,
    config: PathBuf,
    data: PathBuf,
}

impl SandboxFs {
    /// Builds a sandbox over three real directories.
    ///
    /// Each is created if missing and canonicalised once here, so every later
    /// containment check is a prefix test against a path that has already been
    /// resolved — including on Windows, where `canonicalize` returns a `\\?\`
    /// form a plain path would never match.
    pub fn new(
        save: impl AsRef<Path>,
        config: impl AsRef<Path>,
        data: impl AsRef<Path>,
    ) -> Result<Self, FsError> {
        Ok(Self {
            save: path::prepare_root(save.as_ref())?,
            config: path::prepare_root(config.as_ref())?,
            data: path::prepare_root(data.as_ref())?,
        })
    }

    /// The real directory behind a root.
    pub fn root_path(&self, root: Root) -> &Path {
        match root {
            Root::Save => &self.save,
            Root::Config => &self.config,
            Root::Data => &self.data,
        }
    }

    /// Resolves a virtual path for reading.
    pub fn resolve(&self, virtual_path: &str) -> Result<PathBuf, FsError> {
        let parsed = path::parse(virtual_path)?;
        path::descend(self.root_path(parsed.root), &parsed.components, false)
    }

    /// Resolves a virtual path for writing, creating the directories above it.
    ///
    /// Separate from [`Self::resolve`] because it is where the read-only rule
    /// applies: refusing the write at resolution means no caller can perform one
    /// by reaching for the path directly.
    pub fn resolve_for_write(&self, virtual_path: &str) -> Result<PathBuf, FsError> {
        let parsed = path::parse(virtual_path)?;
        if !parsed.root.is_writable() {
            return Err(FsError::ReadOnly(parsed.root));
        }
        path::descend(self.root_path(parsed.root), &parsed.components, true)
    }

    /// Reads a file.
    pub fn read(&self, virtual_path: &str) -> Result<Vec<u8>, FsError> {
        let real = self.resolve(virtual_path)?;
        if !real.is_file() {
            return Err(FsError::NotFound(virtual_path.to_owned()));
        }
        std::fs::read(&real).map_err(|error| FsError::Io(error.to_string()))
    }

    /// Reads a file as UTF-8 text.
    pub fn read_to_string(&self, virtual_path: &str) -> Result<String, FsError> {
        let bytes = self.read(virtual_path)?;
        String::from_utf8(bytes).map_err(|error| FsError::Io(error.to_string()))
    }

    /// Writes a file, replacing what was there.
    pub fn write(&self, virtual_path: &str, contents: impl AsRef<[u8]>) -> Result<(), FsError> {
        let real = self.resolve_for_write(virtual_path)?;
        std::fs::write(&real, contents).map_err(|error| FsError::Io(error.to_string()))
    }

    /// Whether something is there.
    ///
    /// A refused path answers `false` rather than erroring: asking whether
    /// `../../etc/passwd` exists should not be a way to learn that it does.
    pub fn exists(&self, virtual_path: &str) -> bool {
        self.resolve(virtual_path)
            .is_ok_and(|real| real.try_exists().unwrap_or(false))
    }

    /// Deletes a file.
    pub fn delete(&self, virtual_path: &str) -> Result<(), FsError> {
        let real = self.resolve_for_write(virtual_path)?;
        if !real.is_file() {
            return Err(FsError::NotFound(virtual_path.to_owned()));
        }
        std::fs::remove_file(&real).map_err(|error| FsError::Io(error.to_string()))
    }

    /// Lists the entries of a directory, by name, sorted.
    ///
    /// Sorted because directory order is whatever the filesystem happens to
    /// hand back — a game iterating its save slots would show them in a
    /// different order on a different machine.
    pub fn list(&self, virtual_path: &str) -> Result<Vec<String>, FsError> {
        let real = self.resolve(virtual_path)?;
        if !real.is_dir() {
            return Err(FsError::NotFound(virtual_path.to_owned()));
        }

        let mut names: Vec<String> = std::fs::read_dir(&real)
            .map_err(|error| FsError::Io(error.to_string()))?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }
}
