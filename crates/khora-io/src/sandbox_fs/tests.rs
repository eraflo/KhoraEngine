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

//! Sandbox tests.
//!
//! Most of these are escape attempts, and every one of them must **fail**. A
//! sandbox is only worth having if the tests that prove it holds are the ones
//! that would otherwise be a security report.

use std::path::PathBuf;

use tempfile::TempDir;

use super::{FsError, Root, SandboxFs};

/// A sandbox over three temporary directories, plus the outside world for the
/// escape attempts to aim at.
struct Fixture {
    fs: SandboxFs,
    _home: TempDir,
    outside: PathBuf,
}

fn fixture() -> Fixture {
    let home = TempDir::new().expect("a temp dir");
    let base = home.path();

    let outside = base.join("outside");
    std::fs::create_dir_all(&outside).expect("the outside world");
    std::fs::write(outside.join("secrets.txt"), b"do not read me").expect("a secret");

    let fs = SandboxFs::new(base.join("save"), base.join("config"), base.join("data"))
        .expect("a sandbox");

    Fixture {
        fs,
        _home: home,
        outside,
    }
}

// ─── What is meant to work ──────────────────────────────────────────────────

/// **The milestone.** A game has to be able to save.
#[test]
fn a_save_round_trips() {
    let f = fixture();

    f.fs.write("save://slot1.json", b"{\"level\":3}")
        .expect("writes");
    assert!(f.fs.exists("save://slot1.json"));
    assert_eq!(
        f.fs.read_to_string("save://slot1.json").expect("reads"),
        "{\"level\":3}"
    );
}

/// A fresh install has no save directory tree yet, so the first write has to
/// make it rather than fail on it.
#[test]
fn writing_into_a_new_subdirectory_creates_it() {
    let f = fixture();

    f.fs.write("save://profiles/alice/slot1.json", b"hi")
        .expect("writes");
    assert_eq!(
        f.fs.read_to_string("save://profiles/alice/slot1.json")
            .unwrap(),
        "hi"
    );
}

#[test]
fn config_is_writable_too() {
    let f = fixture();
    f.fs.write("config://prefs.ron", b"(volume: 0.8)")
        .expect("writes");
    assert!(f.fs.exists("config://prefs.ron"));
}

#[test]
fn project_data_is_readable() {
    let f = fixture();
    std::fs::write(
        f.fs.root_path(Root::Data).join("weapons.ron"),
        b"(sword: 10)",
    )
    .expect("the project ships this");

    assert_eq!(
        f.fs.read_to_string("data://weapons.ron").expect("reads"),
        "(sword: 10)"
    );
}

#[test]
fn a_file_can_be_deleted() {
    let f = fixture();
    f.fs.write("save://slot1.json", b"x").expect("writes");

    f.fs.delete("save://slot1.json").expect("deletes");
    assert!(!f.fs.exists("save://slot1.json"));
}

/// Directory order is whatever the filesystem hands back, so a game listing its
/// save slots would show them differently on a different machine.
#[test]
fn a_listing_comes_back_sorted() {
    let f = fixture();
    for name in ["c.json", "a.json", "b.json"] {
        f.fs.write(&format!("save://{name}"), b"x").expect("writes");
    }

    assert_eq!(
        f.fs.list("save://").expect("lists"),
        vec!["a.json", "b.json", "c.json"]
    );
}

/// Doubled and trailing separators are a typo, not an attack.
#[test]
fn redundant_separators_name_the_same_file() {
    let f = fixture();
    f.fs.write("save://a/b.json", b"x").expect("writes");

    assert!(f.fs.exists("save://a//b.json"));
    assert!(f.fs.exists("save://./a/b.json"));
}

// ─── What must fail ─────────────────────────────────────────────────────────

/// **The milestone's other half.** The single most familiar escape.
#[test]
fn climbing_out_with_dot_dot_is_refused() {
    let f = fixture();

    let error =
        f.fs.write("save://../../etc/passwd", b"pwned")
            .expect_err("must not escape");
    assert!(matches!(error, FsError::Escapes(_)), "got {error:?}");
    assert!(error.to_string().contains("leaves its root"));
}

#[test]
fn a_single_step_out_is_refused_too() {
    let f = fixture();
    assert!(matches!(
        f.fs.read("save://../outside/secrets.txt"),
        Err(FsError::Escapes(_))
    ));
}

/// An absolute path would make `Path::join` *replace* the root rather than
/// extend it — the one way a join loses its base without erroring.
#[test]
fn an_absolute_path_is_refused() {
    let f = fixture();

    let error =
        f.fs.read("save:///etc/passwd")
            .expect_err("must not escape");
    assert!(matches!(error, FsError::Absolute(_)), "got {error:?}");
}

#[test]
fn a_backslash_is_refused_on_every_platform() {
    let f = fixture();

    // A separator on Windows, an ordinary character elsewhere. A script is
    // written once and shipped everywhere, so it is refused either way.
    assert!(matches!(
        f.fs.write("save://..\\..\\windows\\system32\\x", b"x"),
        Err(FsError::Escapes(_))
    ));
    assert!(matches!(
        f.fs.write("save://sub\\file.json", b"x"),
        Err(FsError::Escapes(_))
    ));
}

/// A drive letter, or an NTFS alternate data stream.
#[test]
fn a_colon_in_a_component_is_refused() {
    let f = fixture();
    assert!(matches!(
        f.fs.write("save://C:/windows/x", b"x"),
        Err(FsError::Escapes(_))
    ));
    assert!(matches!(
        f.fs.write("save://slot1.json:hidden", b"x"),
        Err(FsError::Escapes(_))
    ));
}

/// **The escape no string check can see.** A path of plain names still leaves
/// the root if one of those names is a link pointing out of it — only asking
/// the filesystem where the link goes can catch this.
#[test]
#[cfg(unix)]
fn following_a_symlink_out_of_the_root_is_refused() {
    let f = fixture();
    std::os::unix::fs::symlink(&f.outside, f.fs.root_path(Root::Save).join("escape"))
        .expect("a link out");

    let error =
        f.fs.read("save://escape/secrets.txt")
            .expect_err("must not follow the link out");
    assert!(matches!(error, FsError::Escapes(_)), "got {error:?}");
}

/// The same escape on Windows, where creating a directory symlink needs a
/// privilege a normal user does not have unless Developer Mode is on.
///
/// Written as a fixture check rather than skipped in silence: a test that
/// quietly does nothing reads as coverage it does not provide. When the link
/// cannot be made there is nothing to assert, and the assertion below records
/// that rather than passing as though the escape had been tried.
#[test]
#[cfg(windows)]
fn following_a_symlink_out_of_the_root_is_refused() {
    let f = fixture();
    let link = f.fs.root_path(Root::Save).join("escape");

    let Ok(()) = std::os::windows::fs::symlink_dir(&f.outside, &link) else {
        eprintln!(
            "SKIPPED: creating a directory symlink needs SeCreateSymbolicLinkPrivilege \
             (Developer Mode). The escape itself is covered on unix, which CI runs."
        );
        return;
    };

    let error =
        f.fs.read("save://escape/secrets.txt")
            .expect_err("must not follow the link out");
    assert!(matches!(error, FsError::Escapes(_)), "got {error:?}");
}

/// A script rewriting the game's own content corrupts an installation rather
/// than a save.
#[test]
fn writing_to_project_data_is_refused() {
    let f = fixture();

    let error =
        f.fs.write("data://weapons.ron", b"(sword: 9999)")
            .expect_err("data:// is read-only");
    assert_eq!(error, FsError::ReadOnly(Root::Data));
    assert!(error.to_string().contains("use save:// or config://"));
}

#[test]
fn deleting_from_project_data_is_refused() {
    let f = fixture();
    std::fs::write(f.fs.root_path(Root::Data).join("weapons.ron"), b"x").expect("ships");

    assert_eq!(
        f.fs.delete("data://weapons.ron"),
        Err(FsError::ReadOnly(Root::Data))
    );
    assert!(f.fs.root_path(Root::Data).join("weapons.ron").is_file());
}

// ─── Naming the root ────────────────────────────────────────────────────────

#[test]
fn a_path_with_no_root_says_how_to_write_one() {
    let f = fixture();

    let error = f.fs.read("slot1.json").expect_err("no root named");
    assert!(matches!(error, FsError::NoRoot(_)), "got {error:?}");
    assert!(error.to_string().contains("save://"));
}

#[test]
fn an_unknown_root_lists_the_real_ones() {
    let f = fixture();

    let error = f.fs.read("system://passwd").expect_err("no such root");
    assert_eq!(error, FsError::UnknownRoot("system".to_owned()));
    assert!(error.to_string().contains("config://"));
}

// ─── Behaviour at the edges ─────────────────────────────────────────────────

/// Asking whether `../../etc/passwd` exists should not be a way to learn that
/// it does.
#[test]
fn exists_answers_false_for_a_refused_path_rather_than_erroring() {
    let f = fixture();
    assert!(!f.fs.exists("save://../outside/secrets.txt"));
    assert!(!f.fs.exists("nonsense"));
}

#[test]
fn reading_something_that_is_not_there_says_so() {
    let f = fixture();
    assert!(matches!(
        f.fs.read("save://missing.json"),
        Err(FsError::NotFound(_))
    ));
}

#[test]
fn reading_a_directory_as_a_file_is_not_found() {
    let f = fixture();
    f.fs.write("save://slots/one.json", b"x").expect("writes");

    assert!(matches!(
        f.fs.read("save://slots"),
        Err(FsError::NotFound(_))
    ));
}

#[test]
fn a_write_replaces_what_was_there() {
    let f = fixture();
    f.fs.write("save://slot1.json", b"first").expect("writes");
    f.fs.write("save://slot1.json", b"second").expect("writes");

    assert_eq!(f.fs.read_to_string("save://slot1.json").unwrap(), "second");
}

#[test]
fn roots_are_isolated_from_one_another() {
    let f = fixture();
    f.fs.write("save://x.json", b"save").expect("writes");
    f.fs.write("config://x.json", b"config").expect("writes");

    assert_eq!(f.fs.read_to_string("save://x.json").unwrap(), "save");
    assert_eq!(f.fs.read_to_string("config://x.json").unwrap(), "config");
}

#[test]
fn only_data_is_read_only() {
    assert!(Root::Save.is_writable());
    assert!(Root::Config.is_writable());
    assert!(!Root::Data.is_writable());
}

#[test]
fn a_root_displays_as_a_script_would_write_it() {
    assert_eq!(Root::Save.to_string(), "save://");
    assert_eq!(Root::from_scheme("config"), Some(Root::Config));
    assert_eq!(Root::from_scheme("system"), None);
}
