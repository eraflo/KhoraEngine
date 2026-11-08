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

//! A request to index a project's assets off the frame.

use std::path::PathBuf;

use crate::event::Supersedes;

/// What an index request's outcome is filed under, until a host takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct IndexTicket(pub u64);

/// What the index of an [`IndexRequest`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// A project opened: its index becomes a new asset service, which
    /// replaces the one the engine serves through.
    Open,
    /// The open project's files changed: its index replaces the one the
    /// engine's asset service holds.
    Reindex,
}

/// Index the assets of the project at `project_root` — the files under
/// `<project_root>/assets/`, their identities read from the registry under
/// `<project_root>/.khora/` — for `kind`.
///
/// Filed by a host through the index hub, taken by the `loading` family's
/// lanes. It never coalesces: a reindex filed after another lands after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRequest {
    /// What its outcome is filed under.
    pub ticket: IndexTicket,
    /// The project's root folder.
    pub project_root: PathBuf,
    /// What the index is for.
    pub kind: IndexKind,
}

impl Supersedes for IndexRequest {}
