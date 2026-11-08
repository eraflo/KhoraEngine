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

//! A project's index built off the frame, staged for a frame's boundary.

use std::path::PathBuf;

use crate::event::Supersedes;
use crate::lane::Staged;

use super::{IndexKind, IndexTicket};

/// What indexing one [`IndexRequest`](super::IndexRequest) gave: the index
/// bytes an asset service is built or reindexed from, or why there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedAssets {
    /// The ticket of the request it answers.
    pub ticket: IndexTicket,
    /// What the index is for.
    pub kind: IndexKind,
    /// The project's root folder.
    pub project_root: PathBuf,
    /// The index bytes, or why the project could not be indexed.
    pub index: Result<Vec<u8>, String>,
}

impl Supersedes for IndexedAssets {}

impl Staged for IndexedAssets {}
