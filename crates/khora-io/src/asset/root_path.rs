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

//! One spelling for a root folder, whatever name it was given.
//!
//! A root is named relatively by a mount (`../assets`) and absolutely by the
//! watch that hears of its changes, and whatever is kept per root must be
//! kept under one key. `std::path::absolute` is not that key on every
//! platform: on Windows it folds `..` away, on Unix it keeps it
//! (`/work/crate/../assets`), so one folder would get two keys.

use std::path::{Component, Path, PathBuf};

/// `path` made absolute against the current directory, its `.` and `..`
/// folded away by name — without touching the file system, so a root that
/// does not exist yet, or no longer does, keeps its key. Symbolic links are
/// not followed: two names of one folder through a link stay two names (the
/// watch compares roots by their resolved path for that).
pub fn absolute(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut folded = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            // Above the root there is nothing to climb to: `/..` is `/`.
            Component::ParentDir => {
                if !matches!(
                    folded.components().next_back(),
                    None | Some(Component::RootDir) | Some(Component::Prefix(_))
                ) {
                    folded.pop();
                }
            }
            other => folded.push(other.as_os_str()),
        }
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root named through `..` and the same root named directly are one
    /// key, on every platform.
    #[test]
    fn a_parent_step_is_folded_away() {
        let here = std::env::current_dir().expect("a current directory");
        let through_parent = here.join("assets").join("..").join("assets");
        assert_eq!(absolute(&through_parent), absolute(&here.join("assets")));
    }

    /// A relative name is made absolute against the current directory.
    #[test]
    fn a_relative_name_is_made_absolute() {
        let key = absolute(Path::new("project/./assets"));
        assert!(key.is_absolute(), "{}", key.display());
        assert!(
            key.ends_with(Path::new("project").join("assets")),
            "{}",
            key.display()
        );
        assert!(!key
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir)));
    }

    /// Climbing past the root stays at the root.
    #[test]
    fn climbing_past_the_root_stays_there() {
        let here = std::env::current_dir().expect("a current directory");
        let top = here.ancestors().last().expect("a root").to_path_buf();
        let mut far = top.clone();
        for _ in 0..here.components().count() + 3 {
            far.push("..");
        }
        assert_eq!(absolute(&far.join("assets")), top.join("assets"));
    }

    /// A folder that does not exist keeps its key: nothing is read from disk.
    #[test]
    fn a_missing_folder_keeps_its_key() {
        let key = absolute(Path::new("no-such-folder/../no-such-folder/assets"));
        assert!(
            key.ends_with(Path::new("no-such-folder").join("assets")),
            "{}",
            key.display()
        );
    }
}
