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

//! Walking a tree for the files a project holds.

use std::path::{Path, PathBuf};

use super::temporary::is_temporary_folder;

/// Returns whether a file named `name` is an editor's scratch file, never counted as a project file.
///
/// A backup (`name~`), a lock (`.#name`), a swap file (`.swp`) or a temporary
/// file (`.tmp`), the one [`write_atomic`](super::write_atomic) makes included.
pub fn is_scratch(name: &str) -> bool {
    name.ends_with('~')
        || name.starts_with(".#")
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
}

/// Returns every file under `root`, at any depth, by its path under `root`, sorted.
///
/// Links are not followed: they could lead out of `root`. [Scratch
/// files](is_scratch) are left out, and so is what a [temporary](super::is_temporary)
/// folder under `root` holds. Empty for a root that does not exist; an entry
/// that cannot be read is skipped.
pub fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !is_temporary_folder(entry))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file() && !is_scratch(&entry.file_name().to_string_lossy())
        })
        .filter_map(|entry| entry.path().strip_prefix(root).ok().map(Path::to_path_buf))
        .collect();
    found.sort();
    found
}
