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

//! What the user decided of a project, pinned: the decision kept in a store
//! the hub and the editor share, found by any spelling of the project's
//! folder; the trust of the project open, shared in the runtime; and what
//! trusting a project lifts of the quarantine.
//!
//! Every folder here is a temporary one: no test reads or writes the user's
//! home folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::problems::{ProblemSource, Problems};
use khora_core::Runtime;
use tempfile::TempDir;

use crate::trust::*;

/// A project folder `<dir>/<name>`, made, holding an `assets/` folder.
fn project_in(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(root.join("assets")).expect("the project folder is made");
    root
}

/// The trust store's file under `dir`, as `~/.khora/trust.ron` would be.
fn store_in(dir: &Path) -> PathBuf {
    dir.join("khora").join("trust.ron")
}

// ─── The store ──────────────────────────────────────────────────────────────

/// **A decision is read back from its store.**
///
/// None before one is taken; once decided, the same store answers it, the
/// store's file is written, and a store opened afresh over that file answers it
/// too. A decision taken again replaces the first; another project is still
/// undecided.
#[test]
fn a_decision_is_read_back_from_its_store() {
    let dir = TempDir::new().expect("a temp dir");
    let root = project_in(dir.path(), "ember");
    let other = project_in(dir.path(), "other");
    let path = store_in(dir.path());
    let store = TrustStore::new(Some(path.clone()));

    assert_eq!(store.decision(&root), None, "nothing decided yet");

    store.decide(&root, ProjectTrust::Trusted);

    assert_eq!(store.decision(&root), Some(ProjectTrust::Trusted));
    assert!(path.exists(), "the decision is written to the store's file");
    assert_eq!(
        TrustStore::new(Some(path.clone())).decision(&root),
        Some(ProjectTrust::Trusted),
        "read back from the store's file"
    );

    store.decide(&root, ProjectTrust::Restricted);

    assert_eq!(store.decision(&root), Some(ProjectTrust::Restricted));
    assert_eq!(
        TrustStore::new(Some(path.clone())).decision(&root),
        Some(ProjectTrust::Restricted),
        "the decision taken again replaces the first in the file"
    );
    assert_eq!(
        TrustStore::new(Some(path)).decision(&other),
        None,
        "another project is still undecided"
    );
}

/// **A decision is found by any spelling of the project's folder.**
///
/// It is kept by the folder's canonical path, so the folder named through one
/// of its own sub-folders and `..`, or with a `.` after it, finds the decision
/// taken under its plain name — and the other way round.
#[test]
fn a_decision_is_found_by_any_spelling_of_the_project_folder() {
    let dir = TempDir::new().expect("a temp dir");
    let root = project_in(dir.path(), "ember");
    let second = project_in(dir.path(), "second");
    let path = store_in(dir.path());
    let store = TrustStore::new(Some(path.clone()));

    store.decide(&root, ProjectTrust::Trusted);
    store.decide(&second.join("assets").join(".."), ProjectTrust::Restricted);

    let reread = TrustStore::new(Some(path));
    for store in [&store, &reread] {
        assert_eq!(
            store.decision(&root.join("assets").join("..")),
            Some(ProjectTrust::Trusted),
            "through a sub-folder and `..`"
        );
        assert_eq!(
            store.decision(&root.join(".")),
            Some(ProjectTrust::Trusted),
            "with a `.` after it"
        );
        assert_eq!(
            store.decision(&second),
            Some(ProjectTrust::Restricted),
            "decided under one spelling, found under the plain name"
        );
    }
}

/// **Two stores sharing one file keep each other's decisions.**
///
/// The hub and the editor each open the store, then each decides of its own
/// project. Each writes only its own change over the store as it is then, so
/// the file ends holding both: neither write lost the other's.
#[test]
fn two_stores_sharing_a_file_keep_each_others_decisions() {
    let dir = TempDir::new().expect("a temp dir");
    let ember = project_in(dir.path(), "ember");
    let lantern = project_in(dir.path(), "lantern");
    let path = store_in(dir.path());
    let hub = TrustStore::new(Some(path.clone()));
    let editor = TrustStore::new(Some(path.clone()));

    hub.decide(&ember, ProjectTrust::Trusted);
    editor.decide(&lantern, ProjectTrust::Restricted);

    let reread = TrustStore::new(Some(path.clone()));
    assert_eq!(
        reread.decision(&ember),
        Some(ProjectTrust::Trusted),
        "the first store's decision survived the second's write"
    );
    assert_eq!(reread.decision(&lantern), Some(ProjectTrust::Restricted));

    hub.decide(&lantern, ProjectTrust::Trusted);

    let reread = TrustStore::new(Some(path));
    assert_eq!(
        reread.decision(&lantern),
        Some(ProjectTrust::Trusted),
        "a decision changed by one store replaces the other's"
    );
    assert_eq!(reread.decision(&ember), Some(ProjectTrust::Trusted));
}

/// **A missing or unreadable store reads as no decision.**
///
/// A store whose file does not exist, or holds something else than decisions,
/// decides nothing — and deciding over the unreadable one writes a store that
/// reads. A store with no file keeps its decisions in memory.
#[test]
fn a_missing_or_corrupt_store_reads_as_no_decision() {
    let dir = TempDir::new().expect("a temp dir");
    let root = project_in(dir.path(), "ember");

    let missing = TrustStore::new(Some(dir.path().join("nowhere").join("trust.ron")));
    assert_eq!(missing.decision(&root), None, "no file: no decision");

    let path = store_in(dir.path());
    fs::create_dir_all(path.parent().expect("a parent")).expect("the store's folder");
    fs::write(&path, b"this is not ( a trust store").expect("writes");
    let corrupt = TrustStore::new(Some(path.clone()));
    assert_eq!(
        corrupt.decision(&root),
        None,
        "an unreadable file: no decision"
    );

    corrupt.decide(&root, ProjectTrust::Restricted);
    assert_eq!(
        TrustStore::new(Some(path)).decision(&root),
        Some(ProjectTrust::Restricted),
        "deciding over an unreadable store writes one that reads"
    );

    let in_memory = TrustStore::new(None);
    assert_eq!(in_memory.decision(&root), None);
    in_memory.decide(&root, ProjectTrust::Trusted);
    assert_eq!(
        in_memory.decision(&root),
        Some(ProjectTrust::Trusted),
        "a store with no file keeps its decisions in memory"
    );
}

/// **A decision is read from its file again only when the file changed.**
///
/// The hub asks a store for a project's decision once per card per frame:
/// the store re-reads its file only when its length or its modified time
/// moved since the last read.
///
/// - The file is replaced by bytes that read as no store at all, of the same
///   length and given back the same modified time: the store still answers
///   the decision it read, so it did not read the file again.
/// - The file is given back, then another store — another process sharing
///   it — decides of a second project: the file grew, so the first store
///   reads it again and sees that decision, the first one still there.
#[test]
fn a_decision_is_read_again_only_when_its_file_changed() {
    let dir = TempDir::new().expect("a temp dir");
    let ember = project_in(dir.path(), "ember");
    let lantern = project_in(dir.path(), "lantern");
    let path = store_in(dir.path());
    TrustStore::new(Some(path.clone())).decide(&ember, ProjectTrust::Trusted);
    let hub = TrustStore::new(Some(path.clone()));
    assert_eq!(
        hub.decision(&ember),
        Some(ProjectTrust::Trusted),
        "the premise: the store reads the decision"
    );

    let written = fs::read(&path).expect("the store's file reads");
    let stamp = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .expect("the store's file has a modified time");
    let restamp = |modified: std::time::SystemTime| {
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(modified))
            .expect("the store's file takes its modified time back");
    };
    fs::write(&path, vec![0xFF; written.len()]).expect("the store's file is replaced");
    restamp(stamp);
    assert_eq!(
        fs::metadata(&path).map(|meta| meta.len()).ok(),
        Some(written.len() as u64),
        "the premise: the same length"
    );
    assert_eq!(
        fs::metadata(&path).and_then(|meta| meta.modified()).ok(),
        Some(stamp),
        "the premise: the same modified time"
    );
    assert_eq!(
        TrustStore::new(Some(path.clone())).decision(&ember),
        None,
        "the premise: the bytes in the file read as no decision"
    );

    assert_eq!(
        hub.decision(&ember),
        Some(ProjectTrust::Trusted),
        "a file of the same length and modified time is not read again"
    );

    fs::write(&path, &written).expect("the store's file is given back");
    restamp(stamp);
    TrustStore::new(Some(path.clone())).decide(&lantern, ProjectTrust::Restricted);

    assert_eq!(
        hub.decision(&lantern),
        Some(ProjectTrust::Restricted),
        "a decision written by another store is seen"
    );
    assert_eq!(
        hub.decision(&ember),
        Some(ProjectTrust::Trusted),
        "the decision read before is still there"
    );
}

// ─── The project open ───────────────────────────────────────────────────────

/// **A project's decision is read at its trust**: a trusted project's files
/// at `Project`, a restricted one's at `Restricted`.
#[test]
fn a_project_decision_reads_at_its_trust() {
    assert_eq!(ProjectTrust::Trusted.trust(), Trust::Project);
    assert_eq!(ProjectTrust::Restricted.trust(), Trust::Restricted);
}

/// **The trust of the project open is shared, and changes.**
///
/// The state holds the trust it was made with, then the one set; a runtime
/// holding it reads it, and a runtime holding none — a game, the sandbox, a
/// test — reads as `Trusted`.
#[test]
fn the_project_trust_state_is_shared_through_the_runtime() {
    let state = ProjectTrustState::new(ProjectTrust::Restricted);
    assert_eq!(state.get(), ProjectTrust::Restricted);
    state.set(ProjectTrust::Trusted);
    assert_eq!(state.get(), ProjectTrust::Trusted);
    state.set(ProjectTrust::Restricted);
    assert_eq!(state.get(), ProjectTrust::Restricted);

    assert_eq!(
        ProjectTrustState::of(&Runtime::new()),
        ProjectTrust::Trusted,
        "no state in the runtime reads as trusted"
    );

    let shared = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&shared));
    assert_eq!(ProjectTrustState::of(&runtime), ProjectTrust::Restricted);
    shared.set(ProjectTrust::Trusted);
    assert_eq!(
        ProjectTrustState::of(&runtime),
        ProjectTrust::Trusted,
        "a change of the shared state is read through the runtime"
    );
}

/// **A folder is read at the trust of the project open only when it lies in it.**
///
/// The trust is held for project `game`, trusted:
///
/// - its `assets/`, named as the project was, through a `..`, or by its
///   canonical (`\\?\` on Windows) path, is read trusted;
/// - `game2` beside it — its name starts with `game` — and any other folder
///   are read restricted, so is a folder named relative that is not in it;
/// - the trust set again without naming a project keeps the project named;
/// - a state that names no project — a game's — reads every folder at its
///   trust.
#[test]
fn a_folder_is_read_at_the_trust_of_the_project_open_only_inside_it() {
    let dir = TempDir::new().expect("a temp dir");
    let game = project_in(dir.path(), "game");
    let game2 = project_in(dir.path(), "game2");
    let state = ProjectTrustState::new(ProjectTrust::Restricted);
    state.set_for(&game, ProjectTrust::Trusted);

    let canonical = game.join("assets").canonicalize().expect("canonical");
    for inside in [
        game.join("assets"),
        game.join("..").join("game").join("assets"),
        canonical,
    ] {
        assert_eq!(
            state.trust_of(&inside),
            ProjectTrust::Trusted,
            "{inside:?} lies in the project"
        );
    }
    for outside in [
        game2.join("assets"),
        dir.path().to_path_buf(),
        PathBuf::from("assets"),
    ] {
        assert_eq!(
            state.trust_of(&outside),
            ProjectTrust::Restricted,
            "{outside:?} lies outside the project"
        );
    }

    state.set(ProjectTrust::Trusted);
    assert_eq!(
        state.trust_of(&game2.join("assets")),
        ProjectTrust::Restricted,
        "setting the trust again keeps the project it is the trust of"
    );

    let game_state = ProjectTrustState::new(ProjectTrust::Trusted);
    assert_eq!(
        game_state.trust_of(&game2.join("assets")),
        ProjectTrust::Trusted,
        "a state that names no project reads every folder at its trust"
    );
}

// ─── What trusting a project lifts ──────────────────────────────────────────

/// A refusal of the file `name` under `dir`, at `trust`, for `reason`.
fn refusal(dir: &Path, name: &str, trust: Trust, reason: SecurityReason) -> SecurityRefusal {
    SecurityRefusal {
        path: dir.join("assets").join(name),
        trust,
        layer: TrustLayer::Decoder,
        reason,
    }
}

/// **Trusting a project releases what its restricted limits refused, and
/// nothing else.**
///
/// Of the files quarantined, those refused at `Restricted` for a size
/// (`TooLarge`) or a limit (`OverLimit`) are released — here, in the problems
/// and in the store. A file refused for what it holds (`WrongType`,
/// `DecoderPanic`), one refused as a download (`Imported`) and one refused at a
/// trusted project's own limits (`Project`) stay.
#[test]
fn releasing_the_strict_refusals_keeps_those_refused_for_their_content() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let problems = Arc::new(Problems::default());
    let refusals = Refusals::new(Some(Arc::clone(&problems)), Some(store.clone()));
    let stamp = QuarantineKey::Stamp {
        len: 80 << 20,
        modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
    };

    let too_large = refusal(
        dir.path(),
        "a_too_large.png",
        Trust::Restricted,
        SecurityReason::TooLarge {
            size: 80 << 20,
            cap: 64 << 20,
        },
    );
    let over_limit = refusal(
        dir.path(),
        "b_over_limit.png",
        Trust::Restricted,
        SecurityReason::OverLimit {
            what: "width".to_owned(),
            limit: 8192,
        },
    );
    let wrong_type = refusal(
        dir.path(),
        "c_wrong_type.png",
        Trust::Restricted,
        SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    );
    let panicked = refusal(
        dir.path(),
        "d_panicked.png",
        Trust::Restricted,
        SecurityReason::DecoderPanic {
            message: "boom".to_owned(),
        },
    );
    let imported = refusal(
        dir.path(),
        "e_imported.png",
        Trust::Imported,
        SecurityReason::OverLimit {
            what: "width".to_owned(),
            limit: 8192,
        },
    );
    let at_project = refusal(
        dir.path(),
        "f_at_project.png",
        Trust::Project,
        SecurityReason::OverLimit {
            what: "width".to_owned(),
            limit: 16384,
        },
    );
    refusals.refuse(too_large.clone(), Some(stamp));
    refusals.refuse(over_limit.clone(), Some(QuarantineKey::Content([2; 32])));
    refusals.refuse(wrong_type.clone(), Some(QuarantineKey::Content([3; 32])));
    refusals.refuse(panicked.clone(), Some(QuarantineKey::Content([4; 32])));
    refusals.refuse(imported.clone(), Some(QuarantineKey::Content([5; 32])));
    refusals.refuse(at_project.clone(), Some(QuarantineKey::Content([6; 32])));
    assert_eq!(
        refusals.quarantined().len(),
        6,
        "the premise: six quarantined"
    );

    refusals.release_strict(Trust::Restricted);

    let kept = vec![
        wrong_type.clone(),
        panicked.clone(),
        imported.clone(),
        at_project.clone(),
    ];
    assert_eq!(refusals.quarantined(), kept);
    assert!(
        !refusals.is_quarantined(&too_large.path, &stamp),
        "a file refused too large at the restricted caps reads again"
    );
    assert!(
        !refusals.is_quarantined(&over_limit.path, &QuarantineKey::Content([2; 32])),
        "a file refused over a restricted limit reads again"
    );
    let reported: Vec<String> = problems
        .of_source(&ProblemSource::Security)
        .into_iter()
        .filter_map(|problem| problem.module)
        .collect();
    for released in [&too_large, &over_limit] {
        assert!(
            !reported.contains(&released.path.display().to_string()),
            "a released file's problem is cleared: {reported:?}"
        );
    }
    for stays in &kept {
        assert!(
            reported.contains(&stays.path.display().to_string()),
            "a file still quarantined is still reported: {reported:?}"
        );
    }
    assert_eq!(
        Refusals::new(None, Some(store)).quarantined(),
        kept,
        "the release is written to the store"
    );
}

/// **A decision the store cannot write is still the decision of the run.**
///
/// The store's folder cannot be made (a file stands where it would be), so the
/// decision is not written — yet the same store, asked again, answers it.
/// Otherwise an editor whose user answered "Trust and open" reads the project
/// as undecided at once and asks again, forever.
#[test]
fn a_decision_the_store_cannot_write_is_kept_for_the_run() {
    let dir = TempDir::new().expect("a temp dir");
    let root = project_in(dir.path(), "ember");
    // `khora` is a file: the store's folder cannot be made under it.
    fs::write(dir.path().join("khora"), b"not a folder").expect("writes");
    let store = TrustStore::new(Some(store_in(dir.path())));

    store.decide(&root, ProjectTrust::Trusted);

    assert!(
        !store_in(dir.path()).exists(),
        "the premise: the decision could not be written"
    );
    assert_eq!(
        store.decision(&root),
        Some(ProjectTrust::Trusted),
        "the store answers the decision taken in this run"
    );
}
