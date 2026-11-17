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

use khora_core::sync::LockExt;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::Trust;
use crate::fs::{real_path, FileStamp, Keyed, RonStore};

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

    /// Returns whether the project is opened restricted.
    pub fn is_restricted(self) -> bool {
        self == ProjectTrust::Restricted
    }
}

/// The decision taken for each project, by its canonical path, in a store
/// the hub and the editor share.
///
/// Each decision is written at once, over the store as it is then: a hub
/// and an editor sharing it keep each other's decisions.
pub struct TrustStore {
    store: Option<RonStore<Entry>>,
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
    /// The store file's stamp when it was last read: while it holds, the file
    /// is not read again.
    stamp: Option<FileStamp>,
}

/// One decision as the store keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    project: PathBuf,
    decision: ProjectTrust,
}

impl Keyed for Entry {
    type Key = PathBuf;

    fn key(&self) -> PathBuf {
        self.project.clone()
    }
}

/// The decisions a store's entries hold, by project.
fn decisions(entries: BTreeMap<PathBuf, Entry>) -> HashMap<PathBuf, ProjectTrust> {
    entries
        .into_iter()
        .map(|(project, entry)| (project, entry.decision))
        .collect()
}

impl TrustStore {
    /// Creates a store over the file `store`, read when it exists; in memory only without one.
    pub fn new(store: Option<PathBuf>) -> Self {
        let store = store.map(|path| RonStore::new(path, "trust store"));
        let stamp = store.as_ref().and_then(RonStore::stamp);
        let read = store
            .as_ref()
            .map(|store| decisions(store.read()))
            .unwrap_or_default();
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
        let mut held = self.held.lock_recovered();
        if let Some(store) = &self.store {
            let stamp = store.stamp();
            if stamp.is_none() || stamp != held.stamp {
                held.read = decisions(store.read());
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
        let mut held = self.held.lock_recovered();
        let at = real_path(project_root);
        let Some(store) = &self.store else {
            held.read.insert(at, decision);
            return;
        };
        let entry = Entry {
            project: at.clone(),
            decision,
        };
        let (merged, written) = store.merge([(at.clone(), Some(entry))]);
        match written {
            Ok(()) => {
                held.unwritten.remove(&at);
                held.stamp = store.stamp();
            }
            Err(error) => {
                log::warn!(
                    "Trust: could not write {}: {error}; the decision holds for this run",
                    store.path().display()
                );
                held.unwritten.insert(at, decision);
            }
        }
        held.read = decisions(merged);
    }
}
