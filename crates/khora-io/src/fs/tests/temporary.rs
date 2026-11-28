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

//! Tests of the temporaries made beside an entry and their reclaim.

use super::*;

/// **The names this module gives what it makes beside an entry are temporaries, and only they.**
#[test]
fn is_temporary_holds_for_a_writes_a_builds_and_a_renames_names_only() {
    let made = [
        temporary_beside(Path::new("dir/scene.kscene")),
        previous_beside(Path::new("dist/windows")),
        aside_beside(Path::new("assets/Tex")),
    ];
    for path in &made {
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(is_temporary(&name), "{name}");
    }
    for name in [
        "a.tmp",
        "Tex",
        "v1.2.tmp",
        ".1.2.tmp",
        "a.x.2.tmp",
        "a.1.2.tmp.png",
        "a.1.2.aside",
    ] {
        assert!(!is_temporary(name), "{name}");
    }
}

/// **A rename in case only cut short is put back under its name at the next reclaim.**
///
/// The entry moved aside is put back when nothing holds its name, and left,
/// never removed, when something does.
#[test]
fn reclaim_asides_puts_back_an_entry_whose_name_is_free_and_leaves_one_whose_name_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    let aside = aside_beside(&assets.join("Tex"));
    std::fs::create_dir_all(&aside).unwrap();
    std::fs::write(aside.join("a.png"), b"a").unwrap();
    std::fs::create_dir_all(assets.join("sub")).unwrap();
    std::fs::write(assets.join("sub").join("b.png"), b"kept").unwrap();
    let taken = aside_beside(&assets.join("sub").join("b.png"));
    std::fs::write(&taken, b"aside").unwrap();
    let staged = temporary_beside(&assets.join("c.png"));
    std::fs::write(&staged, b"a write cut short").unwrap();

    let mut reclaimed = reclaim_asides(&assets);
    reclaimed.sort_by_key(|done| matches!(done, Reclaimed::Stranded { .. }));

    assert_eq!(listed(&assets.join("Tex")), ["a.png"]);
    assert!(matches!(
        &reclaimed[0],
        Reclaimed::Restored { from, to } if *from == aside && *to == assets.join("Tex")
    ));
    assert!(matches!(
        &reclaimed[1],
        Reclaimed::Stranded { path, .. } if *path == taken
    ));
    assert_eq!(reclaimed.len(), 2, "{reclaimed:?}");
    assert_eq!(std::fs::read(&taken).unwrap(), b"aside");
    assert_eq!(
        std::fs::read(assets.join("sub").join("b.png")).unwrap(),
        b"kept"
    );
    assert!(staged.exists(), "a write's temporary is no rename's");
}
