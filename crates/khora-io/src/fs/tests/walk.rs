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

//! Tests of walking a tree for a project's files.

use super::*;

#[test]
fn scratch_names_are_backups_locks_swaps_and_temporaries() {
    for name in [
        "guard.erg~",
        ".#guard.erg",
        ".guard.erg.swp",
        "scene.kscene.1234.0.tmp",
        "a.tmp",
    ] {
        assert!(is_scratch(name), "{name} is scratch");
    }
    for name in [
        "guard.erg",
        "~guard.erg",
        ".gitkeep",
        "a.bak",
        "tmp",
        "swp.erg",
        "a.TMP",
    ] {
        assert!(!is_scratch(name), "{name} is not scratch");
    }
}

#[test]
fn the_temporary_a_write_makes_is_scratch() {
    let name = temporary_beside(Path::new("dir/scene.kscene"));
    let name = name.file_name().unwrap().to_string_lossy();
    assert!(is_scratch(&name), "{name}");
}

#[test]
fn files_under_lists_every_file_sorted_by_its_path_under_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("scripts").join("ai")).unwrap();
    std::fs::write(root.join("scripts").join("ai").join("guard.erg"), b"").unwrap();
    std::fs::write(root.join("b.png"), b"").unwrap();
    std::fs::write(root.join("a.png"), b"").unwrap();
    std::fs::create_dir(root.join("empty")).unwrap();
    assert_eq!(
        files_under(root),
        vec![
            PathBuf::from("a.png"),
            PathBuf::from("b.png"),
            Path::new("scripts").join("ai").join("guard.erg"),
        ]
    );
}

#[test]
fn files_under_leaves_scratch_files_out() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in ["guard.erg", "guard.erg~", ".#guard.erg", "a.swp", "a.tmp"] {
        std::fs::write(root.join(name), b"").unwrap();
    }
    assert_eq!(files_under(root), vec![PathBuf::from("guard.erg")]);
}

#[test]
fn files_under_a_root_that_does_not_exist_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(files_under(&dir.path().join("gone")).is_empty());
}

#[cfg(unix)]
#[test]
fn files_under_does_not_follow_links() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"").unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("file-link")).unwrap();
    assert!(files_under(&root).is_empty());
}

/// **What a folder moved aside under a temporary name holds is never walked.**
///
/// A case-only rename moves the folder aside first: a walk meanwhile, or after
/// a crash between the two steps, would count its files under the temporary name.
#[test]
fn a_folder_moved_aside_under_a_temporary_name_is_never_walked() {
    let dir = tempfile::tempdir().unwrap();
    let tex = dir.path().join("Tex");
    std::fs::create_dir(&tex).unwrap();
    std::fs::write(tex.join("a.png"), b"a").unwrap();

    // Where a case-only rename of `Tex` stands between its two steps.
    std::fs::rename(&tex, temporary_beside(&tex)).unwrap();

    assert_eq!(files_under(dir.path()), Vec::<PathBuf>::new());
}
