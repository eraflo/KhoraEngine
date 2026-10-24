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

//! One root, many spellings: a root named relatively, through `..`, with a
//! trailing separator or in another case is the same tree, watched once,
//! reporting its changes, disarmed by any of its names.
//!
//! The editor takes its project from `--project` as typed, so a relative
//! path is an ordinary input, not a corner.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use khora_io::asset::{AssetChangeEvent, AssetWatch};

/// How long a change may take to come back from the OS.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// A fresh directory under the target's scratch space — on the drive the
/// tests run from, so it can be named relatively — removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "khora-io-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `path` named from the current directory, with `..` where they part.
fn relative_from_cwd(path: &Path) -> PathBuf {
    let normal = |path: &Path| -> Vec<std::ffi::OsString> {
        std::fs::canonicalize(path)
            .expect("the path exists")
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_owned()),
                _ => None,
            })
            .collect()
    };
    let here = normal(&std::env::current_dir().expect("a current directory"));
    let there = normal(path);
    let shared = here.iter().zip(&there).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in shared..here.len() {
        relative.push("..");
    }
    for part in &there[shared..] {
        relative.push(part);
    }
    assert!(relative.is_relative(), "{}", relative.display());
    relative
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("the path exists")
}

fn armed() {
    std::thread::sleep(Duration::from_millis(150));
}

fn poll_until(watch: &AssetWatch, rel_path: &str) -> Vec<AssetChangeEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        events.extend(watch.poll_for("probe"));
        if events.iter().any(|event| event.rel_path == rel_path) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    events
}

/// **A root named relatively reports its changes.** `watch` accepts it —
/// it resolves the name — so a save under it must come back, as it does
/// for the same root named absolutely; otherwise a project opened with a
/// relative `--project` hot-reloads nothing, without a word.
#[test]
fn a_root_named_relatively_reports_its_changes() {
    let scratch = Scratch::new("relative-root");
    let relative = relative_from_cwd(scratch.path());
    let watch = AssetWatch::default();
    watch.watch(&relative).expect("a relative root is watched");
    armed();

    std::fs::write(scratch.path().join("wood.png"), b"PNG").expect("writes");
    let events = poll_until(&watch, "wood.png");

    assert!(
        events.iter().any(|event| event.rel_path == "wood.png"),
        "a save under {} was not reported: {events:?}",
        relative.display()
    );
}

/// A root named through `..` is the plain root: watched once, its changes
/// reported, disarmed by the plain name.
#[test]
fn a_root_named_through_dot_dot_is_the_plain_root() {
    let scratch = Scratch::new("dot-dot-root");
    let assets = scratch.path().join("assets");
    std::fs::create_dir_all(scratch.path().join("detour")).expect("a sibling");
    std::fs::create_dir_all(&assets).expect("the root");
    let detour = scratch.path().join("detour").join("..").join("assets");

    let watch = AssetWatch::default();
    watch.watch(&detour).expect("watched through `..`");
    watch
        .watch(&assets)
        .expect("the plain name is the same root");
    assert_eq!(watch.roots().len(), 1, "{:?}", watch.roots());
    armed();

    std::fs::write(assets.join("wood.png"), b"PNG").expect("writes");
    let events = poll_until(&watch, "wood.png");
    let event = events
        .iter()
        .find(|event| event.rel_path == "wood.png")
        .unwrap_or_else(|| panic!("the save was reported: {events:?}"));
    assert_eq!(
        canonical(&event.root().join(&event.rel_path)),
        canonical(&assets.join("wood.png"))
    );

    watch.unwatch(&assets);
    assert!(watch.roots().is_empty(), "{:?}", watch.roots());
}

/// A root named with a trailing separator is the root without it.
#[test]
fn a_root_named_with_a_trailing_separator_is_the_same_root() {
    let scratch = Scratch::new("trailing-root");
    let mut trailing = scratch.path().as_os_str().to_owned();
    trailing.push(std::path::MAIN_SEPARATOR_STR);
    let trailing = PathBuf::from(trailing);

    let watch = AssetWatch::default();
    watch
        .watch(&trailing)
        .expect("watched with a trailing separator");
    watch.watch(scratch.path()).expect("the same root");
    assert_eq!(watch.roots().len(), 1, "{:?}", watch.roots());

    watch.unwatch(scratch.path());
    assert!(watch.roots().is_empty(), "{:?}", watch.roots());
}

/// On a file system that ignores case, a root named in another case is the
/// same root: watched once, disarmed by either name.
#[cfg(windows)]
#[test]
fn a_root_named_in_another_case_is_the_same_root_where_case_is_ignored() {
    let scratch = Scratch::new("case-root");
    let assets = scratch.path().join("assets");
    std::fs::create_dir_all(&assets).expect("the root");
    let shouted = scratch.path().join("ASSETS");

    let watch = AssetWatch::default();
    watch.watch(&shouted).expect("watched in upper case");
    watch.watch(&assets).expect("the same root");
    assert_eq!(watch.roots().len(), 1, "{:?}", watch.roots());
    armed();

    std::fs::write(assets.join("wood.png"), b"PNG").expect("writes");
    let events = poll_until(&watch, "wood.png");
    assert!(
        events.iter().any(|event| event.rel_path == "wood.png"),
        "{events:?}"
    );

    watch.unwatch(&assets);
    assert!(watch.roots().is_empty(), "{:?}", watch.roots());
}

/// **A root is known by its first name, under any spelling**: whatever is kept
/// per root keys on the name the watch reports changes under, not on the
/// spelling a caller happens to hold — or one folder gets two entries.
#[test]
fn a_root_answers_to_its_first_name_under_any_spelling() {
    let scratch = Scratch::new("named-root");
    let watch = AssetWatch::default();
    watch.watch(scratch.path()).expect("watches the root");

    assert_eq!(
        watch.named(scratch.path()).as_deref(),
        Some(scratch.path()),
        "the name it was armed by"
    );
    assert_eq!(
        watch.named(&relative_from_cwd(scratch.path())).as_deref(),
        Some(scratch.path()),
        "another spelling of the same folder"
    );
}

/// A folder that is not watched has no name in the watch.
#[test]
fn a_root_not_watched_has_no_name() {
    let scratch = Scratch::new("unnamed-root");
    let watch = AssetWatch::default();

    assert_eq!(watch.named(scratch.path()), None);
    watch.watch(scratch.path()).expect("watches the root");
    watch.unwatch(scratch.path());
    assert_eq!(watch.named(scratch.path()), None, "unwatched");
}
