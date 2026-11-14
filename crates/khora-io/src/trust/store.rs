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

//! What the user decided of each project: trusted, or opened restricted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::Trust;

/// What the user decided of a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectTrust {
    /// The project is trusted: its files are read at `Trust::Project`, its
    /// scripts run, its shader overlays apply.
    Trusted,
    /// The project is opened restricted: its files are read at
    /// `Trust::Restricted`, nothing of it runs.
    Restricted,
}

impl ProjectTrust {
    /// Returns the trust a file of a project so decided is read at.
    pub fn trust(self) -> Trust {
        match self {
            ProjectTrust::Trusted => Trust::Project,
            ProjectTrust::Restricted => Trust::Restricted,
        }
    }
}

/// The decision taken for each project, by its canonical path, in a store
/// the hub and the editor share.
///
/// Each decision is written at once, over the store as it is then: a hub
/// and an editor sharing it keep each other's decisions.
pub struct TrustStore {
    store: Option<PathBuf>,
    held: Mutex<Held>,
}

/// What a store holds in memory.
#[derive(Default)]
struct Held {
    /// The decisions as the store file last read them.
    read: HashMap<PathBuf, ProjectTrust>,
    /// The decisions this store could not write, kept for the run over what the file says.
    ///
    /// A user is never asked again for a project they answered.
    unwritten: HashMap<PathBuf, ProjectTrust>,
    /// The store file's length and modified time when it was last read:
    /// while they hold, the file is not read again.
    stamp: Option<Stamp>,
}

/// What tells a store file changed: its length and its modified time.
type Stamp = (u64, std::time::SystemTime);

/// The stamp of the store file at `path`; `None` when it has none to read.
fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// One decision as the store keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    project: PathBuf,
    decision: ProjectTrust,
}

/// The largest store read: a few decisions.
const MAX_STORE_BYTES: u64 = 4 << 20;

/// A number for each store write of this process: two writes at once never
/// share their temporary file.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// `path` as the file system resolves it, or as given when it does not.
fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl TrustStore {
    /// Creates a store over the file `store`, read when it exists; in memory only without one.
    pub fn new(store: Option<PathBuf>) -> Self {
        let stamp = store.as_deref().and_then(stamp_of);
        let read = store.as_deref().map(read_store).unwrap_or_default();
        Self {
            store,
            held: Mutex::new(Held {
                read,
                unwritten: HashMap::new(),
                stamp,
            }),
        }
    }

    /// Returns what was decided of the project at `project_root`; `None` when nothing was.
    ///
    /// The file is read again only when its length or time changed, since
    /// another program sharing it may have decided: asking every frame is cheap.
    pub fn decision(&self, project_root: &Path) -> Option<ProjectTrust> {
        let mut held = self.lock();
        if let Some(store) = &self.store {
            let stamp = stamp_of(store);
            if stamp.is_none() || stamp != held.stamp {
                held.read = read_store(store);
                held.stamp = stamp;
            }
        }
        let at = real_path(project_root);
        (held.unwritten.get(&at))
            .or_else(|| held.read.get(&at))
            .copied()
    }

    /// Decides `decision` for the project at `project_root`, written to the
    /// store over what it holds now.
    ///
    /// A decision the store cannot write is logged and holds for this run.
    pub fn decide(&self, project_root: &Path, decision: ProjectTrust) {
        let mut held = self.lock();
        let at = real_path(project_root);
        let Some(store) = &self.store else {
            held.read.insert(at, decision);
            return;
        };
        let mut merged = read_store(store);
        merged.insert(at.clone(), decision);
        match write_store(store, &merged) {
            Ok(()) => {
                held.unwritten.remove(&at);
                held.stamp = stamp_of(store);
            }
            Err(error) => {
                log::warn!(
                    "Trust: could not write {}: {error}; the decision holds for this run",
                    store.display()
                );
                held.unwritten.insert(at, decision);
            }
        }
        held.read = merged;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Held> {
        match self.held.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// The decisions of the store at `path`; none when there is no store, or it
/// does not read (logged).
fn read_store(path: &Path) -> HashMap<PathBuf, ProjectTrust> {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(error) => {
            log::warn!("Trust: {} is not read: {error}", path.display());
            return HashMap::new();
        }
    };
    if size > MAX_STORE_BYTES {
        log::warn!(
            "Trust: {} is {size} bytes, over the {MAX_STORE_BYTES} read",
            path.display()
        );
        return HashMap::new();
    }
    let entries = std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| ron::from_str::<Vec<Entry>>(&text).map_err(|error| error.to_string()));
    match entries {
        Ok(entries) => entries
            .into_iter()
            .map(|entry| (entry.project, entry.decision))
            .collect(),
        Err(error) => {
            log::warn!("Trust: {} is not a trust store: {error}", path.display());
            HashMap::new()
        }
    }
}

/// Writes `decisions` to `path` whole: to a file beside it, then moved over
/// it.
fn write_store(path: &Path, decisions: &HashMap<PathBuf, ProjectTrust>) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut entries: Vec<Entry> = decisions
        .iter()
        .map(|(project, decision)| Entry {
            project: project.clone(),
            decision: *decision,
        })
        .collect();
    entries.sort_by(|a, b| a.project.cmp(&b.project));
    let text = ron::ser::to_string_pretty(&entries, ron::ser::PrettyConfig::default())
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
