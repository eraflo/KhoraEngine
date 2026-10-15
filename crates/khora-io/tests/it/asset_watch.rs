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

//! The engine's one `AssetWatch`: many roots armed and disarmed through
//! `&self`, every change carrying the root it happened under, every
//! subscriber reading every change from its own cursor.
//!
//! The roots are real directories and the changes real writes — the watch is
//! a filesystem watcher, and a fake one would test the fake. Each wait is
//! bounded, as the pump's tests bound theirs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use khora_io::asset::{AssetChangeEvent, AssetChangeKind, AssetWatch};

/// How long a change may take to come back from the OS.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// How long to keep listening once a change came back, so a duplicate that
/// was on its way has arrived too.
const SETTLE: Duration = Duration::from_millis(500);

/// The OS's spelling of `path`, so a root given one way and reported another
/// (`\\?\C:\…` on Windows) compares equal.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|e| panic!("{} exists: {e}", path.display()))
}

/// The watched roots, in the OS's spelling.
fn roots(watch: &AssetWatch) -> BTreeSet<PathBuf> {
    watch.roots().iter().map(|root| canonical(root)).collect()
}

/// Gives the OS a moment to arm what was just watched.
fn armed() {
    std::thread::sleep(Duration::from_millis(150));
}

/// Polls `subscriber` until `done` holds over everything it read, or the wait
/// ran out; returns everything it read either way.
fn poll_until(
    watch: &AssetWatch,
    subscriber: &'static str,
    mut done: impl FnMut(&[AssetChangeEvent]) -> bool,
) -> Vec<AssetChangeEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        events.extend(watch.poll_for(subscriber));
        if done(&events) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    events
}

/// Whether `events` hold one about `rel_path`.
fn mentions(events: &[AssetChangeEvent], rel_path: &str) -> bool {
    events.iter().any(|event| event.rel_path == rel_path)
}

/// How many `Created` in `events` name `rel_path`.
fn created(events: &[AssetChangeEvent], rel_path: &str) -> usize {
    events
        .iter()
        .filter(|event| event.kind == AssetChangeKind::Created && event.rel_path == rel_path)
        .count()
}

// ─── Arming and disarming ───────────────────────────────────────────────────

/// **A root that is not there cannot be watched**, and saying so is the
/// caller's to log: the rest of the application starts.
#[test]
fn watching_a_root_that_does_not_exist_fails_and_watches_nothing() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();

    assert!(watch.watch(&dir.path().join("missing")).is_err());
    assert!(watch.roots().is_empty(), "{:?}", watch.roots());
}

/// The roots are the ones watched and not since unwatched. Unwatching a
/// root that is not watched is already the state asked for, not an error.
#[test]
fn the_roots_are_those_watched_and_not_unwatched() {
    let first = tempfile::TempDir::new().expect("a temp dir");
    let second = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();

    watch.watch(first.path()).expect("watches the first root");
    watch.watch(second.path()).expect("watches the second root");
    assert_eq!(
        roots(&watch),
        BTreeSet::from([canonical(first.path()), canonical(second.path())])
    );

    watch.unwatch(first.path());
    assert_eq!(roots(&watch), BTreeSet::from([canonical(second.path())]));

    watch.unwatch(first.path());
    assert_eq!(
        roots(&watch),
        BTreeSet::from([canonical(second.path())]),
        "unwatching it again changes nothing"
    );
}

/// **Watching a root twice is one watch.** The editor reopens the project it
/// has open, a game and its launcher name the same tree: listed once.
#[test]
fn watching_the_same_root_twice_lists_it_once() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();

    watch.watch(dir.path()).expect("watches the root");
    watch.watch(dir.path()).expect("watching it again succeeds");

    assert_eq!(watch.roots().len(), 1, "{:?}", watch.roots());
}

/// **One watch, not one per call** — and so not counted either: a single
/// `unwatch` disarms a root however many times it was watched.
#[test]
fn one_unwatch_disarms_a_root_watched_twice() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();

    watch.watch(dir.path()).expect("watches the root");
    watch.watch(dir.path()).expect("watching it again succeeds");
    watch.unwatch(dir.path());

    assert!(watch.roots().is_empty(), "{:?}", watch.roots());
}

/// **A root inside a watched root is refused**, naming both: the OS would
/// report every save under it twice, and every save would recompile twice
/// without any test reddening.
#[test]
fn a_root_inside_a_watched_root_is_refused_naming_both() {
    let outer = tempfile::TempDir::new().expect("a temp dir");
    let inner = outer.path().join("inner");
    std::fs::create_dir_all(&inner).expect("makes the inner root");
    let watch = AssetWatch::default();
    watch.watch(outer.path()).expect("watches the outer root");

    let refused = watch
        .watch(&inner)
        .expect_err("a root inside a watched root is refused");

    let message = format!("{refused:#}");
    let outer_name = outer
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a named temp dir");
    assert!(
        message.contains("inner") && message.contains(outer_name),
        "the message names both roots: {message}"
    );
    assert_eq!(
        roots(&watch),
        BTreeSet::from([canonical(outer.path())]),
        "the refused root is not watched"
    );
}

/// The other way round too: a root holding a watched root is refused, and
/// the message names the root already watched inside it.
#[test]
fn a_root_holding_a_watched_root_is_refused_naming_both() {
    let outer = tempfile::TempDir::new().expect("a temp dir");
    let inner = outer.path().join("inner");
    std::fs::create_dir_all(&inner).expect("makes the inner root");
    let watch = AssetWatch::default();
    watch.watch(&inner).expect("watches the inner root");

    let refused = watch
        .watch(outer.path())
        .expect_err("a root holding a watched root is refused");

    let message = format!("{refused:#}");
    let outer_name = outer
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a named temp dir");
    assert!(
        message.contains("inner") && message.contains(outer_name),
        "the message names both roots: {message}"
    );
    assert_eq!(
        roots(&watch),
        BTreeSet::from([canonical(&inner)]),
        "the refused root is not watched"
    );
}

// ─── What subscribers read ──────────────────────────────────────────────────

/// **Each change says which root it happened under.** Two roots, a file
/// written under each: two changes, each carrying its root, and
/// `root().join(rel_path)` is the file that was written.
#[test]
fn each_change_carries_the_root_it_happened_under() {
    let first = tempfile::TempDir::new().expect("a temp dir");
    let second = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();
    watch.watch(first.path()).expect("watches the first root");
    watch.watch(second.path()).expect("watches the second root");
    armed();

    std::fs::write(first.path().join("oak.png"), b"PNG").expect("writes");
    std::fs::write(second.path().join("pine.png"), b"PNG").expect("writes");
    let events = poll_until(&watch, "test", |events| {
        mentions(events, "oak.png") && mentions(events, "pine.png")
    });

    for (dir, rel_path) in [(first.path(), "oak.png"), (second.path(), "pine.png")] {
        let event = events
            .iter()
            .find(|event| event.rel_path == rel_path)
            .unwrap_or_else(|| panic!("a change for {rel_path}: {events:?}"));
        assert_eq!(canonical(event.root()), canonical(dir), "{rel_path}'s root");
        assert_eq!(
            canonical(&event.root().join(&event.rel_path)),
            canonical(&dir.join(rel_path)),
            "the root joined to the relative path is the file written"
        );
    }
}

/// **A root watched twice reports a change once.** Two watches on one tree
/// would deliver every save twice, and a pump would recompile twice.
#[test]
fn a_root_watched_twice_reports_a_change_once() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();
    watch.watch(dir.path()).expect("watches the root");
    watch.watch(dir.path()).expect("watching it again succeeds");
    armed();

    std::fs::write(dir.path().join("oak.png"), b"PNG").expect("writes");
    let mut events = poll_until(&watch, "test", |events| created(events, "oak.png") > 0);
    std::thread::sleep(SETTLE);
    events.extend(watch.poll_for("test"));

    assert_eq!(created(&events, "oak.png"), 1, "{events:?}");
}

/// **Every subscriber reads every change**, each from its own cursor: the
/// editor's VFS reading a change does not take it from the script pump, and
/// a subscriber does not read the same change twice.
#[test]
fn every_subscriber_reads_every_change_once() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();
    watch.watch(dir.path()).expect("watches the root");
    armed();

    std::fs::write(dir.path().join("oak.png"), b"PNG").expect("writes");
    let first = poll_until(&watch, "first", |events| created(events, "oak.png") > 0);
    assert_eq!(created(&first, "oak.png"), 1, "{first:?}");

    let second = poll_until(&watch, "second", |events| created(events, "oak.png") > 0);
    assert_eq!(
        created(&second, "oak.png"),
        1,
        "the second subscriber reads it too: {second:?}"
    );

    assert_eq!(
        created(&watch.poll_for("first"), "oak.png"),
        0,
        "the first subscriber does not read it again"
    );
}

/// **An unwatched root reports nothing more**: what it produced and nobody
/// read goes with it — a closed project has no reloads waiting — and a file
/// written under it afterwards is not reported.
#[test]
fn an_unwatched_root_reports_nothing_more() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let watch = AssetWatch::default();
    watch.watch(dir.path()).expect("watches the root");
    armed();

    std::fs::write(dir.path().join("oak.png"), b"PNG").expect("writes");
    let probe = poll_until(&watch, "probe", |events| mentions(events, "oak.png"));
    assert!(
        mentions(&probe, "oak.png"),
        "the premise: the root reports while watched: {probe:?}"
    );

    watch.unwatch(dir.path());
    let late = watch.poll_for("late");
    assert!(
        !mentions(&late, "oak.png"),
        "an unread change of an unwatched root is gone: {late:?}"
    );

    std::fs::write(dir.path().join("pine.png"), b"PNG").expect("writes");
    std::thread::sleep(SETTLE);
    let after = watch.poll_for("probe");
    assert!(
        !mentions(&after, "pine.png"),
        "a file written after the root was unwatched is not reported: {after:?}"
    );
}
