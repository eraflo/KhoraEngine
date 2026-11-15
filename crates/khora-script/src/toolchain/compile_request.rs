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

/// A request to recompile `modules` of one assets root, with every module
/// importing one of them; a module whose file is gone is withdrawn.
///
/// Taken by the `loading` family's lanes, which merge the waiting requests of a
/// root and compile each root once per run.
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
    /// Creates a request of `root` naming `modules`, which no ticket waits on.
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

    /// Same root, and every module and ticket of `earlier` carried: nothing
    /// is lost, and a request no one waits on never replaces a mount's.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.root == earlier.root
            && self.modules.is_superset(&earlier.modules)
            && self.tickets.is_superset(&earlier.tickets)
    }
}
