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

use super::*;

/// The names of the entries of `dir`, sorted.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn real_path_resolves_a_path_that_exists_and_keeps_one_that_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let spelled = dir.path().join(".").join("x").join("..");
    std::fs::create_dir(dir.path().join("x")).unwrap();
    assert_eq!(
        real_path(&spelled),
        std::fs::canonicalize(dir.path()).unwrap()
    );
    let gone = dir.path().join("gone");
    assert_eq!(real_path(&gone), gone);
}

#[test]
fn relative_to_strips_the_root_as_given() {
    let root = Path::new("not-on-disk/project");
    assert_eq!(
        relative_to(&root.join("assets/a.png"), root),
        Some(PathBuf::from("assets/a.png"))
    );
    assert_eq!(relative_to(root, root), Some(PathBuf::new()));
    assert_eq!(relative_to(Path::new("elsewhere/a.png"), root), None);
}

#[test]
fn relative_to_strips_a_root_spelled_otherwise_once_both_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(assets.join("sub")).unwrap();
    std::fs::write(assets.join("sub").join("a.png"), b"").unwrap();
    let other_spelling = assets.join("sub").join("..");
    assert_eq!(
        relative_to(&assets.join("sub").join("a.png"), &other_spelling),
        Some(PathBuf::from("sub").join("a.png"))
    );
}

/// A path that does not resolve, under a root that does, is under the resolved
/// root as given.
#[test]
fn relative_to_keeps_a_gone_file_under_the_resolved_root() {
    let dir = tempfile::tempdir().unwrap();
    let real_root = std::fs::canonicalize(dir.path()).unwrap();
    let spelled_root = dir.path().join("x").join("..");
    std::fs::create_dir(dir.path().join("x")).unwrap();
    let gone = real_root.join("gone.png");
    assert_eq!(
        relative_to(&gone, &spelled_root),
        Some(PathBuf::from("gone.png"))
    );
}

/// A root that does not resolve is compared as given, against the path as
/// given and resolved.
#[test]
fn relative_to_compares_a_root_that_does_not_resolve_as_given() {
    let dir = tempfile::tempdir().unwrap();
    let gone_root = dir.path().join("gone");
    assert_eq!(
        relative_to(&gone_root.join("a.png"), &gone_root),
        Some(PathBuf::from("a.png"))
    );
    assert_eq!(relative_to(dir.path(), &gone_root), None);
}

#[test]
fn is_under_holds_for_the_root_and_what_is_inside_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    assert!(is_under(&root, &root));
    assert!(is_under(&root.join("a").join("b.txt"), &root));
    assert!(!is_under(dir.path(), &root));
    assert!(
        !is_under(&dir.path().join("project-two"), &root),
        "a sibling sharing the root's name as a prefix is not under it"
    );
}

/// A path that climbs back out of the root through `..` lies outside it,
/// though its first components spell the root.
#[test]
fn a_path_climbing_out_through_a_parent_is_not_under_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(dir.path().join("outside.txt"), b"").unwrap();
    let climbing = root.join("..").join("outside.txt");
    assert_eq!(relative_to(&climbing, &root), None);
    assert!(!is_under(&climbing, &root));
}

/// A file gone from disk, named by the plain path of the folder it was in, is
/// under the root that folder resolves to, whatever spelling the root has.
#[test]
fn relative_to_keeps_a_gone_file_named_plainly_under_a_root_spelled_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("x")).unwrap();
    let spelled_root = dir.path().join("x").join("..");
    let gone = dir.path().join("gone.png");
    assert_eq!(
        relative_to(&gone, &spelled_root),
        Some(PathBuf::from("gone.png"))
    );
}

/// A save over a read-only file is refused and leaves it as it was.
#[test]
fn a_save_over_a_read_only_file_is_refused_and_leaves_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.kscene");
    std::fs::write(&path, b"kept").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    let saved = write_atomic(&path, b"new");
    let kept = std::fs::read(&path).unwrap();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    let _ = std::fs::set_permissions(&path, permissions);
    assert!(saved.is_err(), "the save is refused");
    assert_eq!(kept, b"kept");
    assert_eq!(listed(dir.path()), vec!["scene.kscene"]);
}

#[test]
fn write_atomic_makes_the_folder_and_replaces_the_file_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep").join("scene.kscene");
    write_atomic(&path, b"first, a longer content").unwrap();
    write_atomic(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_eq!(listed(&dir.path().join("deep")), vec!["scene.kscene"]);
}

/// On a file system that ignores case, a save spelled otherwise replaces the
/// file under the name it has: renaming it would give its asset another id.
#[test]
fn a_write_spelled_in_another_case_keeps_the_name_the_file_has() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("turret.kprefab"), b"old").unwrap();
    let other_case = dir.path().join("Turret.kprefab");
    if !other_case.exists() {
        return; // The file system tells case apart: two files, nothing to keep.
    }
    write_atomic(&other_case, b"new").unwrap();
    assert_eq!(listed(dir.path()), vec!["turret.kprefab"]);
    assert_eq!(
        std::fs::read(dir.path().join("turret.kprefab")).unwrap(),
        b"new"
    );
}

#[cfg(unix)]
#[test]
fn a_write_through_a_link_replaces_the_file_it_names_and_keeps_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("real.ron");
    std::fs::write(&file, b"old").unwrap();
    let link = dir.path().join("link.ron");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    write_atomic(&link, b"new").unwrap();
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&file).unwrap(), b"new");
}

#[test]
fn a_failed_write_leaves_the_previous_file_intact_and_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scene.kscene");
    std::fs::write(&path, b"previous").unwrap();
    let failed = write_atomic_with(&path, |file| {
        file.write_all(b"half of the new")?;
        Err(std::io::Error::other("the disk is full"))
    });
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    assert_eq!(listed(dir.path()), vec!["scene.kscene"]);
}

#[test]
fn a_write_whose_rename_fails_removes_its_temporary() {
    let dir = tempfile::tempdir().unwrap();
    // A folder where the file goes: the rename over it fails.
    let path = dir.path().join("taken");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("inside.txt"), b"kept").unwrap();
    assert!(write_atomic(&path, b"new").is_err());
    assert_eq!(listed(dir.path()), vec!["taken"]);
    assert_eq!(std::fs::read(path.join("inside.txt")).unwrap(), b"kept");
}

#[test]
fn writes_at_once_from_many_threads_each_land_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.ron");
    let contents: Vec<Vec<u8>> = (0..8u8).map(|i| vec![b'a' + i; 4096]).collect();
    std::thread::scope(|scope| {
        for bytes in &contents {
            let path = &path;
            scope.spawn(move || {
                // A rename racing another may be refused on Windows; a write
                // that lands, lands whole.
                let _ = write_atomic(path, bytes);
            });
        }
    });
    let landed = std::fs::read(&path).unwrap();
    assert!(contents.contains(&landed), "the file is one write, whole");
    assert_eq!(listed(dir.path()), vec!["shared.ron"]);
}

#[test]
fn write_new_writes_a_file_and_never_replaces_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys").join("pack.key");
    write_new(&path, b"first").unwrap();
    let again = write_new(&path, b"second").unwrap_err();
    assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&path).unwrap(), b"first");
    assert_eq!(listed(&dir.path().join("keys")), vec!["pack.key"]);
}

#[test]
fn write_new_private_writes_a_file_and_never_replaces_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pack.key");
    write_new_private(&path, b"secret").unwrap();
    assert!(write_new_private(&path, b"other").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"secret");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn read_bounded_reads_a_file_at_its_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, [7u8; 16]).unwrap();
    assert_eq!(read_bounded(&path, 16).unwrap(), vec![7u8; 16]);
}

#[test]
fn read_bounded_refuses_a_file_past_its_cap_with_its_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, [7u8; 17]).unwrap();
    match read_bounded(&path, 16) {
        Err(BoundedReadError::TooLarge { size, cap, stamp }) => {
            assert_eq!((size, cap), (17, 16));
            assert_eq!(stamp, FileStamp::of(&path));
        }
        other => panic!("refused as too large, got {other:?}"),
    }
}

#[test]
fn read_bounded_says_a_missing_file_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let error = read_bounded(&dir.path().join("gone"), 16).unwrap_err();
    assert!(error.is_not_found());
}

#[test]
fn read_bounded_to_string_reads_text_and_refuses_what_is_not_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("a.txt");
    std::fs::write(&text, "héllo").unwrap();
    assert_eq!(read_bounded_to_string(&text, 64).unwrap(), "héllo");
    let binary = dir.path().join("a.bin");
    std::fs::write(&binary, [0xFF, 0xFE]).unwrap();
    match read_bounded_to_string(&binary, 64) {
        Err(BoundedReadError::Io(error)) => {
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData)
        }
        other => panic!("not UTF-8 is invalid data, got {other:?}"),
    }
}

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
