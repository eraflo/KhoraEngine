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

//! What a name in a folder reaches when the system opens it, against what the
//! folder lists under that name.

use std::path::Path;

/// What the file system reaches under a name in a folder, checked against the entries the folder lists.
///
/// A name is judged by what the system does with it, never by rules about
/// names: a system that trims a trailing dot, or reads a name as a device,
/// reaches another entry than the one its folder lists, whatever its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reached {
    /// The entry the folder lists under the name, by its spelling on disk
    /// (equal but for case where file names ignore it): the name reaches it.
    Entry(String),
    /// The folder lists nothing under the name, and the name reaches nothing.
    Missing,
    /// The name reaches an entry other than the one the folder lists under it,
    /// or one the folder does not list: a device, a name the system trims.
    Other,
    /// The folder lists an entry under the name, and the name reaches nothing.
    Unreachable,
}

impl Reached {
    /// Returns what opening `name` in the folder `parent` reaches, checked against the folder's listing.
    ///
    /// The entries listed under `name` exactly, then equal but for case, are
    /// each opened through the resolved folder, which reaches them as spelled;
    /// the first that is the entry `parent` joined with `name` reaches is
    /// [`Self::Entry`]. A link is the entry it is, not what it leads to.
    pub(super) fn probe(parent: &Path, name: &str) -> Self {
        let named = parent.join(name);
        let reaches = std::fs::symlink_metadata(&named).is_ok();
        let listed = listed_under(parent, name);
        let none_listed = listed.is_empty();
        let real_parent = khora_sdk::fs::real_path(parent);
        let found = listed
            .into_iter()
            .find(|spelled| khora_sdk::fs::is_same_entry(&named, &real_parent.join(spelled)));
        match found {
            Some(spelled) => Self::Entry(spelled),
            None if reaches => Self::Other,
            None if none_listed => Self::Missing,
            None => Self::Unreachable,
        }
    }

    /// Returns why the name `name` is refused, when it reaches no entry its folder lists under it; `None` for an entry or nothing.
    ///
    /// The sentence names the system and the name, last, between quotes.
    pub(super) fn refusal(&self, name: &str) -> Option<String> {
        let system = if cfg!(windows) {
            "Windows"
        } else {
            "the file system"
        };
        match self {
            Self::Entry(_) | Self::Missing => None,
            Self::Other => Some(format!("{system} reaches another entry under '{name}'")),
            Self::Unreachable => Some(format!(
                "{system} reaches nothing under the name its folder lists, '{name}'"
            )),
        }
    }
}

/// Returns the names the folder `parent` lists that are `name` exactly, then those equal to it but for case.
fn listed_under(parent: &Path, name: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let lower = name.to_lowercase();
    let mut listed: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|spelled| spelled.to_lowercase() == lower)
        .collect();
    listed.sort_by_key(|spelled| spelled != name);
    listed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::io::Result<std::path::PathBuf> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-reached-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn a_name_its_folder_lists_reaches_that_entry_by_its_spelling() -> std::io::Result<()> {
        let root = folder("listed")?;
        std::fs::write(root.join("Level.kscene"), b"x")?;

        assert_eq!(
            Reached::probe(&root, "Level.kscene"),
            Reached::Entry("Level.kscene".to_owned())
        );
        let other_case = Reached::probe(&root, "level.kscene");
        if root.join("level.kscene").exists() {
            assert_eq!(other_case, Reached::Entry("Level.kscene".to_owned()));
        } else {
            assert_eq!(other_case, Reached::Missing);
        }
        assert_eq!(Reached::probe(&root, "gone.kscene"), Reached::Missing);
        assert_eq!(Reached::Entry("a".to_owned()).refusal("a"), None);
        assert_eq!(Reached::Missing.refusal("a"), None);
        Ok(())
    }

    #[test]
    fn a_name_that_reaches_an_entry_its_folder_lists_under_another_name_is_refused(
    ) -> std::io::Result<()> {
        let root = folder("other")?;
        std::fs::create_dir(root.join("v1"))?;

        let probed = Reached::probe(&root, "v1.");

        if root.join("v1.").exists() {
            // Windows trims the dot: the name reaches `v1`.
            assert_eq!(probed, Reached::Other);
            let why = probed.refusal("v1.").expect("refused");
            assert!(why.ends_with("another entry under 'v1.'"), "{why}");
        } else {
            assert_eq!(probed, Reached::Missing);
        }
        Ok(())
    }

    #[test]
    fn a_name_its_folder_lists_and_that_reaches_nothing_is_refused_naming_it() -> std::io::Result<()>
    {
        let root = folder("unreachable")?;
        // Resolved, so that Windows keeps the trailing dot of the name.
        std::fs::create_dir(std::fs::canonicalize(&root)?.join("v1."))?;

        let probed = Reached::probe(&root, "v1.");

        if cfg!(windows) {
            assert_eq!(probed, Reached::Unreachable);
        } else {
            assert_eq!(probed, Reached::Entry("v1.".to_owned()));
        }
        let why = Reached::Unreachable.refusal("v1.").expect("refused");
        assert!(why.ends_with("'v1.'"), "{why}");
        Ok(())
    }
}
