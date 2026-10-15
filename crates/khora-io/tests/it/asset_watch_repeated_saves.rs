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

//! **A second save of the same file is a change of its own**: the watcher
//! collapses the burst of `Modified` one save fires, never the next save —
//! even one of the same length, with nothing else saved in between.

use std::path::Path;
use std::time::{Duration, Instant};

use khora_io::asset::{AssetChangeEvent, AssetChangeKind, AssetWatch};

const MAX_WAIT: Duration = Duration::from_secs(10);

fn modified(events: &[AssetChangeEvent], rel_path: &str) -> usize {
    events
        .iter()
        .filter(|event| event.kind == AssetChangeKind::Modified && event.rel_path == rel_path)
        .count()
}

/// Polls until `rel_path` was modified, or the wait ran out.
fn until_modified(watch: &AssetWatch, rel_path: &str) -> Vec<AssetChangeEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        events.extend(watch.poll_for("repeated_saves"));
        if modified(&events, rel_path) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    events
}

fn save(path: &Path, text: &str) {
    std::fs::write(path, text).expect("saves");
}

#[test]
fn a_second_save_of_the_same_length_is_reported() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let file = assets.path().join("guard.erg");
    save(&file, "fn int N() { return 1; }\n");
    let watch = AssetWatch::default();
    watch.watch(assets.path()).expect("watches the root");
    std::thread::sleep(Duration::from_millis(150));

    save(&file, "fn int N() { return 2; }\n");
    let first = until_modified(&watch, "guard.erg");
    assert!(modified(&first, "guard.erg") > 0, "the premise: {first:?}");
    // Let the first save's burst settle and be read.
    std::thread::sleep(Duration::from_millis(300));
    let _ = watch.poll_for("repeated_saves");

    save(&file, "fn int N() { return 3; }\n");
    let second = until_modified(&watch, "guard.erg");

    assert!(
        modified(&second, "guard.erg") > 0,
        "the second save went unreported: {second:?}"
    );
}
