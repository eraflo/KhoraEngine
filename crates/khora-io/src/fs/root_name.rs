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

//! A root folder as it was named and as the file system resolves it.

use std::path::{Path, PathBuf};

use super::{real_path, relative_to_resolved};

/// A root folder, by the name it was given and the path the file system resolves it to.
///
/// Resolved once: asking whether a path is under it resolves only that path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootName {
    /// As it was named: what a caller lists it by.
    pub named: PathBuf,
    /// As the file system resolves it, or as named when it does not resolve.
    pub resolved: PathBuf,
}

impl RootName {
    /// Returns `named` with its resolved path, or with itself when it does not resolve.
    pub fn of(named: &Path) -> RootName {
        RootName {
            named: named.to_path_buf(),
            resolved: real_path(named),
        }
    }

    /// Returns `named` with its resolved path.
    ///
    /// # Errors
    ///
    /// When `named` does not resolve: it does not exist, or cannot be reached.
    pub fn resolve(named: &Path) -> std::io::Result<RootName> {
        Ok(RootName {
            named: named.to_path_buf(),
            resolved: std::fs::canonicalize(named)?,
        })
    }

    /// Returns whether `root` names this folder: as named, or by any path resolving to it.
    pub fn names(&self, root: &Path) -> bool {
        self.named == root || std::fs::canonicalize(root).is_ok_and(|real| real == self.resolved)
    }

    /// Returns whether `path` is this folder or lies under it, as [`super::relative_to`] decides.
    pub fn holds(&self, path: &Path) -> bool {
        relative_to_resolved(path, &self.named, &self.resolved).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_that_exists_resolves_and_one_that_does_not_keeps_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = RootName::of(dir.path());
        assert_eq!(root.resolved, std::fs::canonicalize(dir.path()).unwrap());
        assert!(RootName::resolve(dir.path()).is_ok());

        let gone = dir.path().join("gone");
        assert_eq!(RootName::of(&gone).resolved, gone);
        assert!(RootName::resolve(&gone).is_err());
    }

    #[test]
    fn a_root_is_named_by_its_name_and_by_another_spelling_of_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        let root = RootName::of(&dir.path().join("assets"));
        assert!(root.names(&dir.path().join("assets")));
        assert!(root.names(&dir.path().join("assets").join("..").join("assets")));
        assert!(!root.names(dir.path()));
    }

    #[test]
    fn a_path_is_held_as_given_or_resolved_and_a_gone_one_by_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(assets.join("a.png"), b"").unwrap();
        let root = RootName::of(&assets);
        assert!(root.holds(&assets.join("a.png")));
        assert!(
            root.holds(&assets.join("gone.png")),
            "a gone file, by its name"
        );
        assert!(root.holds(&root.resolved.join("a.png")));
        assert!(root.holds(&assets.join("..").join("assets").join("a.png")));
        assert!(!root.holds(&dir.path().join("other.png")));
    }
}
