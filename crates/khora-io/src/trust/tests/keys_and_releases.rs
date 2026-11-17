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

//! The quarantine's keys and labels, the refusals a runtime gives, a hand-over
//! chain, the strict releases, and the trust state's generation, pinned.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::Runtime;
use tempfile::TempDir;

use crate::fs::FileStamp;
use crate::trust::*;

/// A refusal of the file at `path`, read at `trust`, for `reason`.
fn refusal(path: &Path, trust: Trust, reason: SecurityReason) -> SecurityRefusal {
    SecurityRefusal {
        path: path.to_path_buf(),
        trust,
        layer: TrustLayer::File,
        reason,
    }
}

fn too_large() -> SecurityReason {
    SecurityReason::TooLarge { size: 2, cap: 1 }
}

fn wrong_type() -> SecurityReason {
    SecurityReason::WrongType {
        expected: "texture".to_owned(),
    }
}

#[test]
fn a_content_key_is_the_blake3_hash_of_the_bytes() {
    assert_eq!(
        QuarantineKey::of_content(b"inside bytes"),
        QuarantineKey::Content(*blake3::hash(b"inside bytes").as_bytes())
    );
    assert_ne!(
        QuarantineKey::of_content(b"one"),
        QuarantineKey::of_content(b"two")
    );
}

/// The label the asset browser shows: the hash's first and last four hex
/// digits, or the words for a stamp.
#[test]
fn a_key_is_labelled_by_the_ends_of_its_hash_or_as_size_and_date() {
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = i as u8;
    }
    assert_eq!(
        QuarantineKey::Content(hash).short_label(),
        "blake3 0001\u{2026}1e1f"
    );
    let stamp = QuarantineKey::Stamp {
        len: 1,
        modified: std::time::SystemTime::UNIX_EPOCH,
    };
    assert_eq!(stamp.short_label(), "size and date");
}

#[test]
fn a_file_stamp_becomes_a_stamp_key() {
    let modified = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(7);
    assert_eq!(
        QuarantineKey::from(FileStamp { len: 42, modified }),
        QuarantineKey::Stamp { len: 42, modified }
    );
}

#[test]
fn only_a_size_or_a_limit_is_a_refusal_trust_lifts() {
    assert!(too_large().is_size_or_limit());
    assert!(SecurityReason::OverLimit {
        what: "pixels".to_owned(),
        limit: 1,
    }
    .is_size_or_limit());
    for reason in [
        SecurityReason::Escape,
        wrong_type(),
        SecurityReason::Quarantined,
        SecurityReason::DecoderPanic {
            message: "boom".to_owned(),
        },
    ] {
        assert!(!reason.is_size_or_limit(), "{reason:?}");
    }
}

#[test]
fn detached_refusals_quarantine_in_memory() {
    let refusals = Refusals::detached();
    let path = PathBuf::from("not-on-disk/assets/liar.png");
    let key = QuarantineKey::of_content(b"lies");
    refusals.refuse(refusal(&path, Trust::Project, wrong_type()), Some(key));
    assert!(refusals.is_quarantined(&path, &key));
    assert!(
        !Refusals::detached().holds(&path),
        "two detached refusals share nothing"
    );
}

#[test]
fn a_runtimes_refusals_are_the_runs_or_detached_ones() {
    let mut runtime = Runtime::new();
    let none = Refusals::of(&runtime);
    assert!(
        !Arc::ptr_eq(&none, &Refusals::of(&runtime)),
        "a runtime holding none gives detached refusals each time"
    );
    let run = Refusals::detached();
    runtime.resources.insert(Arc::clone(&run));
    assert!(Arc::ptr_eq(&run, &Refusals::of(&runtime)));
}

/// Every call of refusals handed over goes to the last of the chain, however
/// long: a guard made before both hand-overs reports to the run's refusals.
#[test]
fn a_call_reaches_the_last_refusals_of_a_hand_over_chain() {
    let (first, second, last) = (
        Refusals::detached(),
        Refusals::detached(),
        Refusals::detached(),
    );
    first.hand_over_to(Arc::clone(&second));
    second.hand_over_to(Arc::clone(&last));
    let path = PathBuf::from("not-on-disk/assets/liar.png");
    let key = QuarantineKey::of_content(b"lies");

    first.refuse(refusal(&path, Trust::Project, wrong_type()), Some(key));
    assert!(last.is_quarantined(&path, &key), "it lands in the last");
    assert!(first.holds(&path) && second.holds(&path), "asked from any");
    assert_eq!(first.quarantine(), last.quarantine());

    first.release(&path);
    assert!(!last.holds(&path), "a release reaches the last too");
}

/// What a strict release under a root lifts: a file refused at that trust
/// for its size, under the root by its real path or — gone from disk — by the
/// path it was refused under, under the root as named or as resolved. Another
/// trust, another reason, another root stay.
#[test]
fn a_strict_release_under_a_root_lifts_only_its_strict_refusals() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("project");
    let other_root = dir.path().join("project-two");
    fs::create_dir_all(root.join("assets")).expect("the project is made");
    fs::create_dir_all(&other_root).expect("the other project is made");
    let present = root.join("assets").join("big.png");
    fs::write(&present, b"").expect("the file is made");
    let gone = root.join("assets").join("gone.png");
    let respelled_root = root.join("assets").join("..");

    let refusals = Refusals::detached();
    let stamp = QuarantineKey::Stamp {
        len: 2,
        modified: std::time::SystemTime::UNIX_EPOCH,
    };
    let refuse = |path: &Path, trust, reason| {
        refusals.refuse(refusal(path, trust, reason), Some(stamp));
    };
    refuse(&present, Trust::Restricted, too_large());
    refuse(&gone, Trust::Restricted, too_large());
    refuse(&root.join("lies.png"), Trust::Restricted, wrong_type());
    refuse(&root.join("imported.png"), Trust::Imported, too_large());
    refuse(&other_root.join("big.png"), Trust::Restricted, too_large());

    let kept = || {
        let mut kept: Vec<PathBuf> = (refusals.quarantined().into_iter())
            .map(|refusal| refusal.path)
            .collect();
        kept.sort();
        kept
    };
    let mut stays = vec![
        root.join("lies.png"),
        root.join("imported.png"),
        other_root.join("big.png"),
    ];

    // Another spelling of the root reaches a file on disk by its real path, and
    // a gone file by the path it was refused under, under the root resolved.
    refusals.release_strict_under(Trust::Restricted, &respelled_root);
    stays.sort();
    assert_eq!(kept(), stays);

    refuse(&gone, Trust::Restricted, too_large());
    refusals.release_strict_under(Trust::Restricted, &root);
    assert_eq!(kept(), stays, "the root as named lifts it too");
}

#[test]
fn a_strict_release_lifts_every_strict_refusal_at_its_trust() {
    let refusals = Refusals::detached();
    let stamp = QuarantineKey::Stamp {
        len: 2,
        modified: std::time::SystemTime::UNIX_EPOCH,
    };
    let at = |name: &str| PathBuf::from("not-on-disk").join(name);
    for (name, trust, reason) in [
        ("a.png", Trust::Restricted, too_large()),
        ("b.png", Trust::Restricted, wrong_type()),
        ("c.png", Trust::Imported, too_large()),
    ] {
        refusals.refuse(refusal(&at(name), trust, reason), Some(stamp));
    }
    refusals.release_strict(Trust::Restricted);
    let kept: Vec<PathBuf> = (refusals.quarantined().into_iter())
        .map(|refusal| refusal.path)
        .collect();
    assert_eq!(kept, vec![at("b.png"), at("c.png")]);
}

/// A guard resolves its root once, but a root made after the guard is read
/// once it exists.
#[test]
fn a_guard_made_before_its_root_exists_reads_once_it_is_made() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let guard = FileGuard::new(&root, Trust::Project, Refusals::detached());
    assert!(matches!(guard.read("a.txt"), Err(ReadError::Io(_))));

    fs::create_dir_all(&root).expect("the root is made");
    fs::write(root.join("a.txt"), b"made").expect("the file is made");
    assert_eq!(guard.read("a.txt").expect("it reads now"), b"made");
    assert_eq!(guard.read("a.txt").expect("and again"), b"made");
}

#[test]
fn a_restricted_project_is_restricted_and_a_runtime_without_state_is_not() {
    assert!(ProjectTrust::Restricted.is_restricted());
    assert!(!ProjectTrust::Trusted.is_restricted());

    let mut runtime = Runtime::new();
    assert!(!ProjectTrustState::is_restricted(&runtime));
    assert_eq!(ProjectTrustState::generation_of(&runtime), 0);
    runtime
        .resources
        .insert(Arc::new(ProjectTrustState::new(ProjectTrust::Restricted)));
    assert!(ProjectTrustState::is_restricted(&runtime));
}

/// The generation moves on every set, the same trust set again included, so
/// a reader comparing it never misses one.
#[test]
fn the_trust_states_generation_moves_on_every_set() {
    let state = ProjectTrustState::new(ProjectTrust::Trusted);
    let start = state.generation();
    state.set(ProjectTrust::Trusted);
    let after_set = state.generation();
    assert_ne!(after_set, start);
    state.set_for(Path::new("not-on-disk/project"), ProjectTrust::Restricted);
    assert_ne!(state.generation(), after_set);
    assert_eq!(state.get(), ProjectTrust::Restricted);
}

/// Two states never share a generation: a reader keyed on it sees a state
/// put in place of another.
#[test]
fn two_trust_states_never_share_a_generation() {
    let first = ProjectTrustState::new(ProjectTrust::Trusted);
    let second = ProjectTrustState::new(ProjectTrust::Restricted);
    assert_ne!(first.generation(), second.generation());
    assert_ne!(first.generation(), 0);
}

#[test]
fn every_remedy_reads_as_plain_sentences() {
    let reasons = [
        SecurityReason::Escape,
        too_large(),
        wrong_type(),
        SecurityReason::Quarantined,
        SecurityReason::OverLimit {
            what: "pixels".to_owned(),
            limit: 1,
        },
        SecurityReason::DecoderPanic {
            message: "boom".to_owned(),
        },
    ];
    for reason in reasons {
        let remedy = reason.remedy();
        assert!(!remedy.contains("  "), "{reason:?}: {remedy:?}");
        assert!(remedy.ends_with('.'), "{reason:?}: {remedy:?}");
    }
}
