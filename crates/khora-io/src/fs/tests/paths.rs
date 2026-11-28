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

//! Tests of the paths as the file system resolves them.

use super::*;

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

/// A file not made yet is spelled as the folder it is named in resolves, the names below it kept.
#[test]
fn spelled_on_disk_resolves_the_folder_that_exists_and_keeps_the_names_below() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("x").join("scenes")).unwrap();
    let spelled = dir
        .path()
        .join("x")
        .join("scenes")
        .join("new")
        .join("a.kscene");
    assert_eq!(
        spelled_on_disk(&spelled),
        std::fs::canonicalize(dir.path().join("x").join("scenes"))
            .unwrap()
            .join("new")
            .join("a.kscene")
    );
    let real = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(spelled_on_disk(dir.path()), real, "a path that exists");
}

/// On a file system that ignores case, a file not made yet, named through
/// the root's folder in another case, is under the root.
#[test]
fn relative_to_finds_a_file_not_made_yet_under_a_root_named_in_another_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    let other_case = PathBuf::from(root.to_string_lossy().to_uppercase());
    if !other_case.exists() || other_case == root {
        return; // The file system tells case apart: another folder.
    }
    assert_eq!(
        relative_to(&other_case.join("scenes").join("b.kscene"), &root),
        Some(PathBuf::from("scenes").join("b.kscene"))
    );
    assert_eq!(
        relative_to(&other_case.join("SCENES").join("b.kscene"), &root),
        Some(PathBuf::from("scenes").join("b.kscene")),
        "a folder under the root is named as it is spelled on disk"
    );
}

#[test]
fn resolved_relative_to_names_a_path_by_what_exists_of_it_and_refuses_one_resolving_outside() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("assets");
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    let real_root = std::fs::canonicalize(&root).unwrap();

    assert_eq!(
        resolved_relative_to(&root.join("scenes").join("new.kscene"), &real_root),
        Some(Path::new("scenes").join("new.kscene"))
    );
    assert_eq!(
        resolved_relative_to(&root.join("..").join("assets").join("a.png"), &real_root),
        Some(PathBuf::from("a.png"))
    );
    assert_eq!(
        resolved_relative_to(&dir.path().join("other.png"), &real_root),
        None
    );
    if ignores_case(dir.path()) {
        assert_eq!(
            resolved_relative_to(&root.join("SCENES").join("new.kscene"), &real_root),
            Some(Path::new("scenes").join("new.kscene")),
            "a folder in another case is named as it is spelled"
        );
    }
}

/// **Two hard links to one file are two entries**: a rename of one onto the other is no rename in case only.
#[test]
fn two_hard_links_to_one_file_are_two_entries() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    std::fs::write(&a, b"a").unwrap();
    if std::fs::hard_link(&a, &b).is_err() {
        return; // The file system makes no hard link.
    }

    assert!(
        !is_same_entry(&a, &b),
        "two names of one file taken as one entry"
    );
}

/// **Two names of one folder are one entry only when they differ in case alone.**
#[test]
fn is_same_entry_needs_one_folder_names_equal_but_for_case_and_one_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("a.png"), b"a").unwrap();
    assert!(is_same_entry(
        &dir.path().join("a.png"),
        &dir.path().join("a.png")
    ));
    assert!(is_same_entry(
        &dir.path().join("a.png"),
        &dir.path().join("sub").join("..").join("a.png")
    ));
    assert!(!is_same_entry(
        &dir.path().join("a.png"),
        &dir.path().join("sub").join("a.png")
    ));
    assert!(!is_same_entry(
        &dir.path().join("a.png"),
        &dir.path().join("gone.png")
    ));
}
