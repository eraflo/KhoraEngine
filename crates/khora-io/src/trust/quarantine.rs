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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use serde::{Deserialize, Serialize};

use super::{SecurityReason, SecurityRefusal};

/// What a quarantined file is held by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuarantineKey {
    /// The BLAKE3 hash of its bytes, when they were read.
    Content([u8; 32]),
    /// Its stamp, when it was refused before it was read.
    Stamp {
        /// Its length, in bytes.
        len: u64,
        /// When it was last modified.
        modified: std::time::SystemTime,
    },
}

/// Every refusal of a run, and the files quarantined.
///
/// Shared by every reader of a run: the guards, the asset reader. A refusal
/// is reported to the problems under [`ProblemSource::Security`]; one with a
/// key — what the file held, or its stamp — quarantines the file:
/// - a content refused stays refused while the file holds it; once the file
///   reads with another content it is no longer reported, but the content
///   refused is refused again should it come back;
/// - a file refused by its stamp (too large to read) leaves the quarantine as
///   soon as it reads.
///
/// An `Escape` is never quarantined: the path is refused whatever it holds.
/// Files are known by their real path, however a root spells them.
///
/// The quarantine is kept in a store several runs may share (two editors,
/// the editor and the sandbox): each write reads it again and keeps what the
/// others quarantined.
pub struct Refusals {
    problems: Option<Arc<Problems>>,
    store: Option<PathBuf>,
    held: Mutex<Held>,
    /// The refusals these were handed over to: every call goes there.
    successor: Mutex<Option<Arc<Refusals>>>,
}

/// What the refusals hold.
#[derive(Default)]
struct Held {
    /// The files quarantined, by their real path.
    quarantined: HashMap<PathBuf, Entry>,
    /// The refusals reported that quarantine nothing, each once.
    reported: Vec<SecurityRefusal>,
    /// What these refusals changed since they last wrote the store, by real
    /// path: the entry they put there, or `None` for one they took out. A
    /// write applies only these to the store as it is then — another run
    /// sharing it keeps what it changed.
    changed: HashMap<PathBuf, Option<Entry>>,
}

/// One quarantined file, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    refusal: SecurityRefusal,
    key: QuarantineKey,
    /// Its real path, what it is held by; empty in a store from before.
    #[serde(default)]
    at: PathBuf,
    /// The file has read with another content since: it is not reported,
    /// and the content refused is refused again should it come back.
    #[serde(default)]
    superseded: bool,
}

impl Entry {
    /// The path the entry is held by.
    fn place(&self) -> PathBuf {
        if self.at.as_os_str().is_empty() {
            real_path(&self.refusal.path)
        } else {
            self.at.clone()
        }
    }
}

/// The largest store read: a quarantine list is a few entries.
const MAX_STORE_BYTES: u64 = 4 << 20;

/// The most refusals kept that quarantine nothing: past it, the oldest go.
const MAX_REPORTED: usize = 256;

/// A number for each store write of this process: two writes at once never
/// share their temporary file.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// `path` as the file system resolves it, or as given when it does not.
fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl std::fmt::Debug for Refusals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Refusals")
            .field("store", &self.store)
            .field("quarantined", &self.lock().quarantined.len())
            .finish()
    }
}

impl Refusals {
    /// Refusals reported to `problems`, the quarantine kept in `store` (read
    /// when it exists, written on each change).
    pub fn new(problems: Option<Arc<Problems>>, store: Option<PathBuf>) -> Self {
        let quarantined = store
            .as_deref()
            .map(read_store)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.place(), entry))
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
        refusals.report(&refusals.lock());
        refusals
    }

    /// Hands what these refusals hold over to `successor`, and every call
    /// after: what read through these — a guard made before the run's
    /// refusals were known — reports to the run's.
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
        let held = std::mem::take(&mut *self.lock());
        for entry in held.quarantined.into_values() {
            successor.adopt(entry);
        }
        for refusal in held.reported {
            successor.refuse(refusal, None);
        }
        if let Some(problems) = &self.problems {
            problems.report(ProblemSource::Security, Vec::new());
        }
        *self
            .successor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(successor);
    }

    /// Holds `entry` as it is — superseded or not — reporting and storing
    /// it: what a hand-over brings.
    fn adopt(&self, entry: Entry) {
        if let Some(successor) = self.successor() {
            return successor.adopt(entry);
        }
        let mut held = self.lock();
        let at = entry.place();
        held.changed.insert(at.clone(), Some(entry.clone()));
        held.quarantined.insert(at, entry);
        self.write(&mut held);
        self.report(&held);
    }

    /// The refusals these were handed over to, if any.
    fn successor(&self) -> Option<Arc<Refusals>> {
        self.successor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Reports `refusal`, and quarantines its file by `key`.
    pub fn refuse(&self, refusal: SecurityRefusal, key: Option<QuarantineKey>) {
        if let Some(successor) = self.successor() {
            return successor.refuse(refusal, key);
        }
        log::warn!("Refused: {refusal}");
        let mut held = self.lock();
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
                self.write(&mut held);
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
        self.report(&held);
    }

    /// Whether the file at `abs_path`, as `key` says it is now, is
    /// quarantined.
    pub fn is_quarantined(&self, abs_path: &Path, key: &QuarantineKey) -> bool {
        if let Some(successor) = self.successor() {
            return successor.is_quarantined(abs_path, key);
        }
        let held = self.lock();
        find(&held, abs_path).is_some_and(|(_, entry)| entry.key == *key)
    }

    /// Whether anything is quarantined at `abs_path`, whatever it holds: a
    /// reader hashes a file's bytes only when it is.
    pub fn holds(&self, abs_path: &Path) -> bool {
        if let Some(successor) = self.successor() {
            return successor.holds(abs_path);
        }
        find(&self.lock(), abs_path).is_some()
    }

    /// Notes that the file at `abs_path` read, as `key` says it is now: a
    /// file quarantined by its stamp fits now and leaves the quarantine; one
    /// quarantined by another content is no longer reported.
    pub fn note_read(&self, abs_path: &Path, key: &QuarantineKey) {
        if let Some(successor) = self.successor() {
            return successor.note_read(abs_path, key);
        }
        let mut held = self.lock();
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
        self.write(&mut held);
        self.report(&held);
    }

    /// Notes that the file at `abs_path` holds again the content it was
    /// quarantined for: it is reported again.
    pub fn note_refused_again(&self, abs_path: &Path) {
        if let Some(successor) = self.successor() {
            return successor.note_refused_again(abs_path);
        }
        let mut held = self.lock();
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
        self.write(&mut held);
        self.report(&held);
    }

    /// Lets the file at `abs_path` be read again.
    pub fn release(&self, abs_path: &Path) {
        if let Some(successor) = self.successor() {
            return successor.release(abs_path);
        }
        let mut held = self.lock();
        let Some(at) = find(&held, abs_path).map(|(at, _)| at) else {
            return;
        };
        held.quarantined.remove(&at);
        held.changed.insert(at, None);
        self.write(&mut held);
        self.report(&held);
    }

    /// Releases the refusals made at `trust` for a size or a limit
    /// (`TooLarge`, `OverLimit`): what trusting a project lifts. A file
    /// refused for what it holds (`WrongType`, `DecoderPanic`), or refused at
    /// another trust, stays.
    pub fn release_strict(&self, trust: super::Trust) {
        if let Some(successor) = self.successor() {
            return successor.release_strict(trust);
        }
        let mut held = self.lock();
        let lifted: Vec<PathBuf> = (held.quarantined.iter())
            .filter(|(_, entry)| {
                entry.refusal.trust == trust
                    && matches!(
                        entry.refusal.reason,
                        SecurityReason::TooLarge { .. } | SecurityReason::OverLimit { .. }
                    )
            })
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

    /// Releases the refusals made at `trust` for a size or a limit, of the
    /// files under `root` only — trusting one project lifts its own
    /// refusals, never another project's.
    pub fn release_strict_under(&self, trust: super::Trust, root: &Path) {
        if let Some(successor) = self.successor() {
            return successor.release_strict_under(trust, root);
        }
        let real_root = real_path(root);
        // A file is under the project by its real path, or — gone from disk
        // since it was refused — by the path it was refused under.
        let under = |at: &Path, entry: &Entry| {
            at.starts_with(&real_root)
                || entry.refusal.path.starts_with(root)
                || real_path(&entry.refusal.path).starts_with(&real_root)
        };
        let mut held = self.lock();
        let lifted: Vec<PathBuf> = (held.quarantined.iter())
            .filter(|(at, entry)| {
                under(at, entry)
                    && entry.refusal.trust == trust
                    && matches!(
                        entry.refusal.reason,
                        SecurityReason::TooLarge { .. } | SecurityReason::OverLimit { .. }
                    )
            })
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

    /// The refusals quarantined, but those whose file has read with another
    /// content since.
    pub fn quarantined(&self) -> Vec<SecurityRefusal> {
        if let Some(successor) = self.successor() {
            return successor.quarantined();
        }
        let mut refusals: Vec<SecurityRefusal> = (self.lock().quarantined.values())
            .filter(|entry| !entry.superseded)
            .map(|entry| entry.refusal.clone())
            .collect();
        refusals.sort_by(|a, b| a.path.cmp(&b.path));
        refusals
    }

    /// The refusals quarantined, as [`quarantined`](Self::quarantined) lists
    /// them, each with the key its file is held by — what the editor shows
    /// of a quarantined file.
    pub fn quarantine(&self) -> Vec<(SecurityRefusal, QuarantineKey)> {
        if let Some(successor) = self.successor() {
            return successor.quarantine();
        }
        let mut quarantine: Vec<(SecurityRefusal, QuarantineKey)> =
            (self.lock().quarantined.values())
                .filter(|entry| !entry.superseded)
                .map(|entry| (entry.refusal.clone(), entry.key))
                .collect();
        quarantine.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        quarantine
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Held> {
        // A reader that panicked holding the lock left whole entries.
        match self.held.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        }
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

    /// Writes what these refusals changed to the store, when there is one:
    /// the store as it is now, another run's changes kept, with these
    /// applied over it. What is held becomes the store as written.
    fn write(&self, held: &mut Held) {
        let changed = std::mem::take(&mut held.changed);
        let Some(store) = &self.store else {
            return;
        };
        let mut merged: HashMap<PathBuf, Entry> = read_store(store)
            .into_iter()
            .map(|entry| (entry.place(), entry))
            .collect();
        for (at, entry) in changed {
            match entry {
                Some(entry) => merged.insert(at, entry),
                None => merged.remove(&at),
            };
        }
        held.quarantined = merged;
        let mut entries: Vec<&Entry> = held.quarantined.values().collect();
        entries.sort_by(|a, b| a.at.cmp(&b.at));
        if let Err(error) = write_store(store, &entries) {
            log::warn!("Quarantine: could not write {}: {error}", store.display());
        }
    }
}

/// The entry held for `abs_path` — by its real path, else by the path a
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

/// The problem a refusal is reported as.
fn problem_of(refusal: &SecurityRefusal) -> Problem {
    Problem {
        severity: ProblemSeverity::Error,
        module: Some(refusal.path.display().to_string()),
        line: 0,
        column: 0,
        message: format!("Refused: the file {}", refusal.reason),
        note: Some(refusal.reason.remedy().to_owned()),
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

/// The entries of the store at `path`; none when there is no store, or it
/// does not read (logged).
fn read_store(path: &Path) -> Vec<Entry> {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            log::warn!("Quarantine: {} is not read: {error}", path.display());
            return Vec::new();
        }
    };
    if size > MAX_STORE_BYTES {
        log::warn!(
            "Quarantine: {} is {size} bytes, over the {MAX_STORE_BYTES} read",
            path.display()
        );
        return Vec::new();
    }
    match std::fs::read_to_string(path).map(|text| ron::from_str::<Vec<Entry>>(&text)) {
        Ok(Ok(entries)) => entries,
        Ok(Err(error)) => {
            log::warn!(
                "Quarantine: {} is not a quarantine list: {error}",
                path.display()
            );
            Vec::new()
        }
        Err(error) => {
            log::warn!("Quarantine: {} is not read: {error}", path.display());
            Vec::new()
        }
    }
}

/// Writes `entries` to `path` whole: to a file beside it, then moved over it.
fn write_store(path: &Path, entries: &[&Entry]) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let text = ron::ser::to_string_pretty(entries, ron::ser::PrettyConfig::default())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path)
}
