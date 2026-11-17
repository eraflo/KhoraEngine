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

//! The files refused: reported to the problems, and quarantined until their
//! content changes or the user releases them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::sync::LockExt;
use khora_core::Runtime;
use serde::{Deserialize, Serialize};

use crate::fs::{real_path, FileStamp, Keyed, RonStore, RootName};

use super::{SecurityReason, SecurityRefusal};

/// The most refusals kept that quarantine nothing: past it, the oldest go.
const MAX_REPORTED: usize = 256;

/// What a quarantined file is held by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuarantineKey {
    /// The BLAKE3 hash of its bytes, when they were read: refused while the file holds them.
    Content([u8; 32]),
    /// Its stamp, when it was refused before it was read: released once the file reads whole.
    Stamp {
        /// Its length, in bytes.
        len: u64,
        /// When it was last modified.
        modified: std::time::SystemTime,
    },
}

impl QuarantineKey {
    /// Returns the key of a file holding `bytes`: their BLAKE3 hash.
    pub fn of_content(bytes: &[u8]) -> QuarantineKey {
        QuarantineKey::Content(*blake3::hash(bytes).as_bytes())
    }

    /// Returns what the key holds a file by, in a few words: its hash shortened, or "size and date".
    pub fn short_label(&self) -> String {
        match self {
            QuarantineKey::Content(hash) => {
                let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("blake3 {}\u{2026}{}", &hex[..4], &hex[hex.len() - 4..])
            }
            QuarantineKey::Stamp { .. } => "size and date".to_owned(),
        }
    }
}

impl From<FileStamp> for QuarantineKey {
    fn from(stamp: FileStamp) -> Self {
        QuarantineKey::Stamp {
            len: stamp.len,
            modified: stamp.modified,
        }
    }
}

/// Every refusal of a run, and the files quarantined.
///
/// Shared by every reader of a run. A refusal is reported to the problems
/// under [`ProblemSource::Security`]; one with a key quarantines its file, in
/// a store several runs may share.
///
/// See the book: concepts/trust.md, "The quarantine".
pub struct Refusals {
    problems: Option<Arc<Problems>>,
    store: Option<RonStore<Entry>>,
    // A reader that panicked holding the lock left whole entries.
    held: Mutex<Held>,
    /// The refusals these were handed over to: every call goes there.
    successor: Mutex<Option<Arc<Refusals>>>,
}

impl std::fmt::Debug for Refusals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Refusals")
            .field("store", &self.store.as_ref().map(RonStore::path))
            .field("quarantined", &self.held.lock_recovered().quarantined.len())
            .finish()
    }
}

impl Refusals {
    /// Creates refusals reported to `problems`, the quarantine kept in `store`.
    ///
    /// The store is read when it exists, and written on each change.
    pub fn new(problems: Option<Arc<Problems>>, store: Option<PathBuf>) -> Self {
        let store = store.map(|path| RonStore::new(path, "quarantine list"));
        let quarantined = store
            .as_ref()
            .map(RonStore::read)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let refusals = Self {
            problems,
            store,
            held: Mutex::new(Held {
                quarantined,
                ..Held::default()
            }),
            successor: Mutex::new(None),
        };
        refusals.report(&refusals.held.lock_recovered());
        refusals
    }

    /// Returns the run's refusals `runtime` holds, or [`detached`](Self::detached) ones when it holds none.
    pub fn of(runtime: &Runtime) -> Arc<Refusals> {
        runtime
            .resources
            .get::<Arc<Refusals>>()
            .cloned()
            .unwrap_or_else(Self::detached)
    }

    /// Returns refusals of no run: reported to no problems, quarantined in memory only.
    ///
    /// For a test, or a tool reading with no run of its own.
    pub fn detached() -> Arc<Refusals> {
        Arc::new(Self::new(None, None))
    }

    /// Hands what these refusals hold, and every later call, over to `successor`.
    ///
    /// A guard made before the run's refusals were known then reports to the run's.
    pub fn hand_over_to(&self, successor: Arc<Refusals>) {
        // Handing over to refusals that hand over to these would loop.
        let mut next = Some(Arc::clone(&successor));
        while let Some(refusals) = next {
            if std::ptr::eq(self, refusals.as_ref()) {
                log::warn!("Refusals: a hand-over back to refusals handed over to is ignored");
                return;
            }
            next = refusals.successor();
        }
        let held = std::mem::take(&mut *self.held.lock_recovered());
        for entry in held.quarantined.into_values() {
            successor.adopt(entry);
        }
        for refusal in held.reported {
            successor.refuse(refusal, None);
        }
        if let Some(problems) = &self.problems {
            problems.report(ProblemSource::Security, Vec::new());
        }
        *self.successor.lock_recovered() = Some(successor);
    }

    /// Reports `refusal`, and quarantines its file by `key`.
    pub fn refuse(&self, refusal: SecurityRefusal, key: Option<QuarantineKey>) {
        let this = self.current();
        log::warn!("Refused: {refusal}");
        let mut held = this.held.lock_recovered();
        match key.filter(|_| refusal.reason != SecurityReason::Escape) {
            Some(key) => {
                let at = real_path(&refusal.path);
                let entry = Entry {
                    refusal,
                    key,
                    at: at.clone(),
                    superseded: false,
                };
                if held.quarantined.get(&at) == Some(&entry) {
                    return;
                }
                held.changed.insert(at.clone(), Some(entry.clone()));
                held.quarantined.insert(at, entry);
                this.write(&mut held);
            }
            None => {
                if held.reported.contains(&refusal) {
                    return;
                }
                if held.reported.len() == MAX_REPORTED {
                    held.reported.remove(0);
                }
                held.reported.push(refusal);
            }
        }
        this.report(&held);
    }

    /// Returns whether the file at `abs_path`, as `key` says it is now, is
    /// quarantined.
    pub fn is_quarantined(&self, abs_path: &Path, key: &QuarantineKey) -> bool {
        let this = self.current();
        let held = this.held.lock_recovered();
        find(&held, abs_path).is_some_and(|(_, entry)| entry.key == *key)
    }

    /// Returns whether anything is quarantined at `abs_path`, whatever it holds.
    ///
    /// A reader hashes a file's bytes only when it is.
    pub fn holds(&self, abs_path: &Path) -> bool {
        let this = self.current();
        let held = this.held.lock_recovered();
        find(&held, abs_path).is_some()
    }

    /// Notes that the file at `abs_path` read, as `key` says it is now.
    ///
    /// A file quarantined by its stamp leaves the quarantine; one quarantined
    /// for another content is set aside, and stops being reported.
    pub fn note_read(&self, abs_path: &Path, key: &QuarantineKey) {
        let this = self.current();
        let mut held = this.held.lock_recovered();
        let Some((at, entry)) = find(&held, abs_path).map(|(at, entry)| (at, entry.clone())) else {
            return;
        };
        match entry.key {
            QuarantineKey::Stamp { .. } => {
                held.quarantined.remove(&at);
                held.changed.insert(at, None);
            }
            QuarantineKey::Content(_) if entry.key != *key && !entry.superseded => {
                let superseded = Entry {
                    superseded: true,
                    ..entry
                };
                held.changed.insert(at.clone(), Some(superseded.clone()));
                held.quarantined.insert(at, superseded);
            }
            QuarantineKey::Content(_) => return,
        }
        this.write(&mut held);
        this.report(&held);
    }

    /// Notes that the file at `abs_path` holds again the content it was
    /// quarantined for: it is reported again.
    pub(crate) fn note_refused_again(&self, abs_path: &Path) {
        let this = self.current();
        let mut held = this.held.lock_recovered();
        let Some(at) = find(&held, abs_path)
            .filter(|(_, entry)| entry.superseded)
            .map(|(at, _)| at)
        else {
            return;
        };
        if let Some(entry) = held.quarantined.get_mut(&at) {
            entry.superseded = false;
            let entry = entry.clone();
            held.changed.insert(at, Some(entry));
        }
        this.write(&mut held);
        this.report(&held);
    }

    /// Lets the file at `abs_path` be read again.
    pub fn release(&self, abs_path: &Path) {
        let this = self.current();
        let mut held = this.held.lock_recovered();
        let Some(at) = find(&held, abs_path).map(|(at, _)| at) else {
            return;
        };
        held.quarantined.remove(&at);
        held.changed.insert(at, None);
        this.write(&mut held);
        this.report(&held);
    }

    /// Releases the refusals made at `trust` for a size or a limit (`TooLarge`, `OverLimit`).
    ///
    /// A file refused for what it holds (`WrongType`, `DecoderPanic`), or at
    /// another trust, stays.
    pub fn release_strict(&self, trust: super::Trust) {
        self.current()
            .release_where(|_, entry| entry.is_strict_at(trust));
    }

    /// Releases the refusals made at `trust` for a size or a limit, of the files under `root` only.
    pub fn release_strict_under(&self, trust: super::Trust, root: &Path) {
        // A file is under the project by its real path, or — gone from disk
        // since it was refused — by the path it was refused under.
        let root = RootName::of(root);
        self.current().release_where(|at, entry| {
            (at.starts_with(&root.resolved) || root.holds(&entry.refusal.path))
                && entry.is_strict_at(trust)
        });
    }

    /// Returns the refusals quarantined, but those whose file has read with another content since.
    pub fn quarantined(&self) -> Vec<SecurityRefusal> {
        (self.quarantine().into_iter())
            .map(|(refusal, _)| refusal)
            .collect()
    }

    /// Returns the refusals [`quarantined`](Self::quarantined) lists, each with its file's key.
    pub fn quarantine(&self) -> Vec<(SecurityRefusal, QuarantineKey)> {
        let this = self.current();
        let mut quarantine: Vec<(SecurityRefusal, QuarantineKey)> =
            (this.held.lock_recovered().quarantined.values())
                .filter(|entry| !entry.superseded)
                .map(|entry| (entry.refusal.clone(), entry.key))
                .collect();
        quarantine.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        quarantine
    }

    /// Returns the refusals every call acts on: the last of the chain these were handed over to.
    fn current(&self) -> Current<'_> {
        let Some(mut last) = self.successor() else {
            return Current::Here(self);
        };
        while let Some(next) = last.successor() {
            last = next;
        }
        Current::HandedOver(last)
    }

    /// Returns the refusals these were handed over to, if any.
    fn successor(&self) -> Option<Arc<Refusals>> {
        self.successor.lock_recovered().clone()
    }

    /// Holds `entry` as a hand-over brings it, superseded or not, and reports and stores it.
    fn adopt(&self, entry: Entry) {
        let this = self.current();
        let mut held = this.held.lock_recovered();
        let at = entry.place();
        held.changed.insert(at.clone(), Some(entry.clone()));
        held.quarantined.insert(at, entry);
        this.write(&mut held);
        this.report(&held);
    }

    /// Releases the quarantined files `lift` picks, by the path each is held by.
    fn release_where(&self, lift: impl Fn(&Path, &Entry) -> bool) {
        let mut held = self.held.lock_recovered();
        let lifted: Vec<PathBuf> = (held.quarantined.iter())
            .filter(|(at, entry)| lift(at, entry))
            .map(|(at, _)| at.clone())
            .collect();
        if lifted.is_empty() {
            return;
        }
        for at in lifted {
            held.quarantined.remove(&at);
            held.changed.insert(at, None);
        }
        self.write(&mut held);
        self.report(&held);
    }

    /// Reports every refusal held to the problems, replacing the last list.
    fn report(&self, held: &Held) {
        let Some(problems) = &self.problems else {
            return;
        };
        let mut quarantined: Vec<&SecurityRefusal> = (held.quarantined.values())
            .filter(|entry| !entry.superseded)
            .map(|entry| &entry.refusal)
            .collect();
        quarantined.sort_by(|a, b| a.path.cmp(&b.path));
        let list = quarantined
            .into_iter()
            .chain(held.reported.iter())
            .map(problem_of)
            .collect();
        problems.report(ProblemSource::Security, list);
    }

    /// Writes what these refusals changed over the store as it is now, when there is one.
    ///
    /// What is held becomes the store as written, another run's changes included.
    fn write(&self, held: &mut Held) {
        let changed = std::mem::take(&mut held.changed);
        let Some(store) = &self.store else {
            return;
        };
        let (merged, written) = store.merge(changed);
        held.quarantined = merged.into_iter().collect();
        if let Err(error) = written {
            log::warn!(
                "Quarantine: could not write {}: {error}",
                store.path().display()
            );
        }
    }
}

/// What the refusals hold.
#[derive(Default)]
struct Held {
    /// The files quarantined, by their real path.
    quarantined: HashMap<PathBuf, Entry>,
    /// The refusals reported that quarantine nothing, each once.
    reported: Vec<SecurityRefusal>,
    /// What these refusals changed since they last wrote the store, by real
    /// path: the entry put there, or `None` for one taken out.
    ///
    /// A write applies only these, so another run sharing the store keeps its own.
    changed: HashMap<PathBuf, Option<Entry>>,
}

/// One quarantined file, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    refusal: SecurityRefusal,
    key: QuarantineKey,
    /// Its real path, what it is held by; empty in a store that does not record it.
    #[serde(default)]
    at: PathBuf,
    /// The file has read with another content since: it is not reported,
    /// and the content refused is refused again should it come back.
    #[serde(default)]
    superseded: bool,
}

impl Entry {
    /// Returns the path the entry is held by.
    fn place(&self) -> PathBuf {
        if self.at.as_os_str().is_empty() {
            real_path(&self.refusal.path)
        } else {
            self.at.clone()
        }
    }

    /// Returns whether the entry was refused at `trust` for its size or a limit.
    fn is_strict_at(&self, trust: super::Trust) -> bool {
        self.refusal.trust == trust && self.refusal.reason.is_size_or_limit()
    }
}

impl Keyed for Entry {
    type Key = PathBuf;

    fn key(&self) -> PathBuf {
        self.place()
    }
}

/// The refusals a call acts on: these, or the last these were handed over to.
enum Current<'a> {
    Here(&'a Refusals),
    HandedOver(Arc<Refusals>),
}

impl std::ops::Deref for Current<'_> {
    type Target = Refusals;

    fn deref(&self) -> &Refusals {
        match self {
            Current::Here(refusals) => refusals,
            Current::HandedOver(refusals) => refusals,
        }
    }
}

/// Finds the entry held for `abs_path` — by its real path, else by the path a
/// refusal named it by — with the path it is held by.
fn find<'a>(held: &'a Held, abs_path: &Path) -> Option<(PathBuf, &'a Entry)> {
    let at = real_path(abs_path);
    if let Some(entry) = held.quarantined.get(&at) {
        return Some((at, entry));
    }
    held.quarantined
        .iter()
        .find(|(_, entry)| entry.refusal.path == abs_path)
        .map(|(at, entry)| (at.clone(), entry))
}

/// Returns the problem a refusal is reported as.
fn problem_of(refusal: &SecurityRefusal) -> Problem {
    Problem {
        note: Some(refusal.reason.remedy().to_owned()),
        ..Problem::of_file(
            ProblemSeverity::Error,
            refusal.path.display().to_string(),
            0,
            0,
            format!("Refused: the file {}", refusal.reason),
        )
    }
}
