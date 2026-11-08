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

//! A request to recompile modules of one project root.

use std::collections::BTreeSet;
use std::path::PathBuf;

use khora_core::event::Supersedes;

use super::CompileTicket;

/// Recompile `modules` of the assets root `root` — and every module
/// importing one of them; a module whose file is gone is withdrawn.
///
/// Filed by the hot-reload pump, taken by the `loading` family's lanes. A newer
/// request of a root replaces a waiting one only when it names every module
/// the waiting one does (nothing is lost); the requests of one root that
/// remain are merged by the lane, which compiles each root once per run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileRequest {
    /// The assets root whose script folder holds the modules.
    pub root: PathBuf,
    /// The modules changed, by the path they are known by
    /// (`ai/guard.erg`).
    pub modules: BTreeSet<String>,
    /// The tickets of the hosts waiting on this compile — a mount's — empty
    /// for a request no one waits on (the hot-reload pump's).
    pub tickets: BTreeSet<CompileTicket>,
}

impl CompileRequest {
    /// A request of `root` naming `modules`, which no ticket waits on.
    pub fn new(root: impl Into<PathBuf>, modules: impl IntoIterator<Item = String>) -> Self {
        Self {
            root: root.into(),
            modules: modules.into_iter().collect(),
            tickets: BTreeSet::new(),
        }
    }
}

impl Supersedes for CompileRequest {
    const COALESCES: bool = true;

    /// The same root, every module `earlier` names named again, and every
    /// ticket waiting on it carried: a request no one waits on never takes
    /// the place of a mount's.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.root == earlier.root
            && self.modules.is_superset(&earlier.modules)
            && self.tickets.is_superset(&earlier.tickets)
    }
}
