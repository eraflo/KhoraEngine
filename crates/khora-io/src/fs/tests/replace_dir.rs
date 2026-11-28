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

//! Tests of building a folder whole.

use super::*;

/// **A folder built whole replaces the previous one**: what the previous
/// held and the new build does not is gone, and no temporary folder is left.
#[test]
fn a_folder_built_whole_replaces_the_previous_one() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    std::fs::create_dir_all(&target).expect("the previous build");
    std::fs::write(target.join("stale.bin"), b"old").expect("writes");

    let built = replace_dir_with(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")?;
        Ok::<_, std::io::Error>(7)
    });

    assert_eq!(built.expect("the build lands"), 7);
    assert_eq!(listed(&target), ["game.exe"]);
    assert_eq!(
        listed(dir.path()),
        ["windows"],
        "no temporary folder is left"
    );
}

/// **A failed build leaves the previous folder whole**, and its temporary folder is removed.
#[test]
fn a_failed_build_leaves_the_previous_folder_whole() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    std::fs::create_dir_all(&target).expect("the previous build");
    std::fs::write(target.join("game.exe"), b"old").expect("writes");

    let built = replace_dir_with(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"half")?;
        Err::<(), _>(std::io::Error::other("the build failed"))
    });

    assert!(built.is_err());
    assert_eq!(
        std::fs::read(target.join("game.exe")).expect("reads"),
        b"old"
    );
    assert_eq!(
        listed(dir.path()),
        ["windows"],
        "no temporary folder is left"
    );
}

/// **A folder built where none was is made.**
#[test]
fn a_folder_built_where_none_was_is_made() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("dist").join("linux");

    replace_dir_with(&target, |staged| {
        std::fs::write(staged.join("game"), b"new")
    })
    .expect("the build lands");

    assert_eq!(listed(&target), ["game"]);
}

/// **A build reclaims what an interrupted build left beside its folder.**
///
/// A build killed while it filled its temporary folder leaves that folder; one
/// killed between its two renames leaves the previous build under a scratch
/// name and no folder at the target. The next build lands, and neither is left.
#[test]
fn a_build_reclaims_what_an_interrupted_build_left_beside_its_folder() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    let staged = dir.path().join("windows.4242.0.tmp");
    std::fs::create_dir_all(&staged).expect("a killed build's temporary folder");
    std::fs::write(staged.join("game.exe"), b"half").expect("writes");
    let previous = dir.path().join("windows.4242.1.tmp");
    std::fs::create_dir_all(&previous).expect("the previous build moved aside");
    std::fs::write(previous.join("game.exe"), b"old").expect("writes");

    replace_dir_with(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")
    })
    .expect("the build lands");

    assert_eq!(listed(&target), ["game.exe"]);
    assert_eq!(
        listed(dir.path()),
        ["windows"],
        "what the interrupted build left is still beside the folder"
    );
}

/// **A folder held open by another program is left as it was**, and the build says so.
///
/// A running game keeps its executable open without letting it be deleted:
/// the swap fails, the previous build stays whole, and no temporary folder is left.
#[cfg(windows)]
#[test]
fn a_folder_held_open_is_left_as_it_was() {
    use std::os::windows::fs::OpenOptionsExt;
    /// Lets others read the file, never delete or rename it: how a running executable is held.
    const FILE_SHARE_READ: u32 = 0x1;
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    std::fs::create_dir_all(&target).expect("the previous build");
    std::fs::write(target.join("game.exe"), b"old").expect("writes");
    let running = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(target.join("game.exe"))
        .expect("the game runs");

    let built = replace_dir_with(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")
    });
    let left = listed(dir.path());
    drop(running);

    assert!(built.is_err(), "the swap fails: {built:?}");
    assert_eq!(
        std::fs::read(target.join("game.exe")).expect("reads"),
        b"old"
    );
    assert_eq!(left, ["windows"], "no temporary folder is left");
}

/// **A build puts back the previous build an interrupted build moved aside.**
///
/// Killed between its two renames, a build leaves no folder at the target and
/// the previous build under its moved-aside name. The next build puts it back
/// first, so a build that then fails leaves the previous build whole, and says so.
#[test]
fn a_build_puts_back_the_previous_build_an_interrupted_build_moved_aside() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    let previous = dir.path().join("windows.4242.1.previous.tmp");
    std::fs::create_dir_all(&previous).expect("the previous build moved aside");
    std::fs::write(previous.join("game.exe"), b"old").expect("writes");

    let (built, reclaimed) = replace_dir_with_reporting(&target, |_staged| {
        Err::<(), _>(std::io::Error::other("the build failed"))
    });

    assert!(built.is_err());
    assert_eq!(
        std::fs::read(target.join("game.exe")).expect("the previous build is back"),
        b"old"
    );
    assert_eq!(listed(dir.path()), ["windows"]);
    assert_eq!(reclaimed, [Reclaimed::PutBack { from: previous }]);
}

/// **A build reports each leftover it removed.**
#[test]
fn a_build_reports_each_leftover_it_removed() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    std::fs::create_dir_all(&target).expect("the previous build");
    let staged = dir.path().join("windows.4242.0.tmp");
    let previous = dir.path().join("windows.4242.1.previous.tmp");
    std::fs::create_dir_all(&staged).expect("a killed build's temporary folder");
    std::fs::create_dir_all(&previous).expect("a previous build left behind");

    let (built, mut reclaimed) = replace_dir_with_reporting(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")
    });

    built.expect("the build lands");
    reclaimed.sort_by_key(ToString::to_string);
    let mut expected = vec![
        Reclaimed::Removed { path: staged },
        Reclaimed::Removed { path: previous },
    ];
    expected.sort_by_key(ToString::to_string);
    assert_eq!(reclaimed, expected, "the folder at the target is kept");
    assert_eq!(listed(&target), ["game.exe"]);
    assert_eq!(listed(dir.path()), ["windows"]);
}

/// **A build never touches a folder another pattern names.**
///
/// Only a folder named after the target by the build's own pattern is a
/// leftover: a backup the user made, another target's leftover, a near name
/// and a file of the pattern are kept.
#[test]
fn a_build_never_touches_a_folder_another_pattern_names() {
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    let kept = [
        "windows-old",
        "windows.backup",
        "windows.12.tmp",
        "windows.a.1.tmp",
        "windows.1.2.tmp.keep",
        "windowsx.1.2.tmp",
        "linux.1.2.tmp",
        "windows.1.2.old.tmp",
    ];
    for name in kept {
        std::fs::create_dir_all(dir.path().join(name)).expect("a folder of the user's");
    }
    std::fs::write(dir.path().join("windows.7.8.tmp"), b"a file").expect("a file");

    let (built, reclaimed) = replace_dir_with_reporting(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")
    });

    built.expect("the build lands");
    assert!(reclaimed.is_empty(), "{reclaimed:?}");
    let mut expected: Vec<String> = kept.iter().map(|name| (*name).to_owned()).collect();
    expected.extend(["windows".to_owned(), "windows.7.8.tmp".to_owned()]);
    expected.sort();
    assert_eq!(listed(dir.path()), expected);
}

/// **A leftover that cannot be removed is reported left, and the build lands.**
#[cfg(windows)]
#[test]
fn a_leftover_that_cannot_be_removed_is_reported_left() {
    use std::os::windows::fs::OpenOptionsExt;
    /// Lets others read the file, never delete or rename it.
    const FILE_SHARE_READ: u32 = 0x1;
    let dir = tempfile::TempDir::new().expect("a temporary folder");
    let target = dir.path().join("windows");
    let staged = dir.path().join("windows.4242.0.tmp");
    std::fs::create_dir_all(&staged).expect("a killed build's temporary folder");
    std::fs::write(staged.join("game.exe"), b"half").expect("writes");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(staged.join("game.exe"))
        .expect("another program holds it");

    let (built, reclaimed) = replace_dir_with_reporting(&target, |staged| {
        std::fs::write(staged.join("game.exe"), b"new")
    });
    drop(held);

    built.expect("the build lands");
    assert_eq!(listed(&target), ["game.exe"]);
    assert!(
        matches!(reclaimed.as_slice(), [Reclaimed::Left { path, .. }] if *path == staged),
        "{reclaimed:?}"
    );
}
