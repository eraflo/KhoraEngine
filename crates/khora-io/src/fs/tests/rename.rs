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

//! Tests of renaming an entry.

use super::*;

#[test]
fn a_case_only_rename_is_the_same_entry_and_renames_it() {
    let dir = tempfile::tempdir().unwrap();
    if !ignores_case(dir.path()) {
        return; // Two names, two entries.
    }
    std::fs::create_dir(dir.path().join("Tex")).unwrap();
    std::fs::write(dir.path().join("Tex").join("a.png"), b"a").unwrap();
    assert!(is_same_entry(
        &dir.path().join("Tex"),
        &dir.path().join("tex")
    ));
    assert!(!is_same_entry(
        &dir.path().join("Tex"),
        &dir.path().join("Other")
    ));

    rename_entry(&dir.path().join("Tex"), &dir.path().join("tex")).unwrap();

    assert_eq!(listed(dir.path()), ["tex"]);
    assert_eq!(listed(&dir.path().join("tex")), ["a.png"]);
}

#[test]
fn rename_entry_moves_an_entry_and_leaves_one_named_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"a").unwrap();
    rename_entry(&dir.path().join("a.png"), &dir.path().join("a.png")).unwrap();
    assert_eq!(listed(dir.path()), ["a.png"]);

    rename_entry(&dir.path().join("a.png"), &dir.path().join("b.png")).unwrap();
    assert_eq!(listed(dir.path()), ["b.png"]);
    assert!(!is_same_entry(
        &dir.path().join("b.png"),
        &dir.path().join("a.png")
    ));
}

/// **A case-only rename moves its entry aside under a rename's own name**, which a reclaim knows.
#[test]
fn a_case_only_rename_cut_short_leaves_an_entry_a_reclaim_puts_back() {
    let dir = tempfile::tempdir().unwrap();
    let tex = dir.path().join("Tex");
    std::fs::create_dir(&tex).unwrap();
    let aside = aside_beside(&tex);
    let name = aside.file_name().unwrap().to_string_lossy();
    assert!(matches!(
        temporary_of(&name),
        Some(("Tex", Leftover::Aside))
    ));
    std::fs::rename(&tex, &aside).unwrap();

    assert_eq!(
        reclaim_asides(dir.path()),
        [Reclaimed::Restored {
            from: aside,
            to: tex
        }]
    );
    assert_eq!(listed(dir.path()), ["Tex"]);
}
