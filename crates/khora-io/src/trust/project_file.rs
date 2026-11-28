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

//! The files of a project's root, outside `assets/`, that the engine and its
//! tools read.

use std::path::{Path, PathBuf};

use super::FileKind;
use crate::asset::{REGISTRY_DIR, REGISTRY_FILE};

/// A file of a project's root, outside `assets/`, read through a guard over that root.
///
/// Each is read under the cap of its own [`FileKind`]
/// ([`FileGuard::read_project_file`](super::FileGuard::read_project_file)).
/// See the book: concepts/trust.md, "Files of the project root".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectFile {
    /// `project.json`: the game's name, version, engine version and settings.
    Manifest,
    /// `.khora/asset-registry.ron`: the UUID frozen for each renamed asset.
    IdentityRegistry,
    /// `.git/HEAD`: the branch checked out.
    GitHead,
    /// `.gitignore`.
    GitIgnore,
}

impl ProjectFile {
    /// Returns the file's path under the project root.
    pub fn rel_path(self) -> PathBuf {
        match self {
            ProjectFile::Manifest => PathBuf::from("project.json"),
            ProjectFile::IdentityRegistry => Path::new(REGISTRY_DIR).join(REGISTRY_FILE),
            ProjectFile::GitHead => Path::new(".git").join("HEAD"),
            ProjectFile::GitIgnore => PathBuf::from(".gitignore"),
        }
    }

    /// Returns the cap class the file is read under.
    pub fn kind(self) -> FileKind {
        match self {
            ProjectFile::Manifest => FileKind::Manifest,
            ProjectFile::IdentityRegistry => FileKind::Registry,
            ProjectFile::GitHead => FileKind::GitHead,
            ProjectFile::GitIgnore => FileKind::GitIgnore,
        }
    }
}
