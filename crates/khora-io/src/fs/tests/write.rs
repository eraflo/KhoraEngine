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

//! Tests of writing a file whole.

use super::*;

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
