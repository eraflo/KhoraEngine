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

//! **The quarantine shows what each file is held by.** The editor's asset
//! browser shows, for a quarantined file, whether its content or its stamp
//! keeps it there: `Refusals::quarantine` lists what `quarantined` lists,
//! each refusal with its key.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::trust::{QuarantineKey, Refusals, SecurityReason, SecurityRefusal, Trust, TrustLayer};

/// A folder under the temp folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-io-quarantine-view-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }

    /// A file named `name` in this folder, written.
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, b"bytes").expect("the file");
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn too_large(path: PathBuf) -> SecurityRefusal {
    SecurityRefusal {
        path,
        trust: Trust::Restricted,
        layer: TrustLayer::File,
        reason: SecurityReason::TooLarge {
            size: 80 << 20,
            cap: 64 << 20,
        },
    }
}

fn wrong_type(path: PathBuf) -> SecurityRefusal {
    SecurityRefusal {
        path,
        trust: Trust::Project,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    }
}

fn stamp() -> QuarantineKey {
    QuarantineKey::Stamp {
        len: 80 << 20,
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    }
}

/// **Each quarantined refusal comes with its key**: a file refused for its
/// content is held by its hash, one refused before it was read by its
/// stamp. What quarantines nothing — a refusal with no key, an escape — is
/// not listed, as `quarantined` does not list it.
#[test]
fn the_quarantine_lists_each_refusal_with_its_key() {
    let folder = Folder::new("keys");
    let huge = too_large(folder.file("huge.png"));
    let liar = wrong_type(folder.file("liar.png"));
    let refusals = Refusals::new(None, None);
    refusals.refuse(huge.clone(), Some(stamp()));
    refusals.refuse(liar.clone(), Some(QuarantineKey::Content([0xab; 32])));
    refusals.refuse(wrong_type(folder.file("unkeyed.png")), None);
    refusals.refuse(
        SecurityRefusal {
            path: folder.0.join("..").join("outside.png"),
            trust: Trust::Project,
            layer: TrustLayer::File,
            reason: SecurityReason::Escape,
        },
        Some(QuarantineKey::Content([1; 32])),
    );

    let mut listed = refusals.quarantine();
    listed.sort_by(|a, b| a.0.path.cmp(&b.0.path));

    assert_eq!(
        listed,
        vec![(huge, stamp()), (liar, QuarantineKey::Content([0xab; 32])),],
        "each quarantined refusal, with what it is held by"
    );
    assert_eq!(
        listed
            .iter()
            .map(|(refusal, _)| refusal.clone())
            .collect::<Vec<_>>(),
        {
            let mut quarantined = refusals.quarantined();
            quarantined.sort_by(|a, b| a.path.cmp(&b.path));
            quarantined
        },
        "it lists what `quarantined` lists"
    );
}

/// **A file read with another content since is not listed**, as
/// `quarantined` does not list it; a file released is gone from the list.
#[test]
fn the_quarantine_leaves_out_a_file_read_again_or_released() {
    let folder = Folder::new("left-out");
    let liar = folder.file("liar.png");
    let huge = folder.file("huge.png");
    let refusals = Refusals::new(None, None);
    refusals.refuse(
        wrong_type(liar.clone()),
        Some(QuarantineKey::Content([2; 32])),
    );
    refusals.refuse(too_large(huge.clone()), Some(stamp()));
    assert_eq!(refusals.quarantine().len(), 2, "the premise: both are held");

    refusals.note_read(&liar, &QuarantineKey::Content([9; 32]));
    refusals.release(&huge);

    assert!(
        refusals.quarantine().is_empty(),
        "nothing is listed: {:?}",
        refusals.quarantine()
    );
}
