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

//! What a compile of one root came to, by the tickets it answers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use khora_core::event::Supersedes;
use khora_core::lane::Staged;

use super::CompileTicket;

/// One root compiled by the `loading` family: the tickets of every request
/// merged into that compile, and what it came to — how many modules
/// compiled, or why the compile failed.
///
/// Staged under the root's lease, beside the reloads and declarations it
/// produced; a boundary files its outcome under each of its tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileDone {
    /// The assets root compiled.
    pub root: PathBuf,
    /// The tickets of the requests merged into the compile.
    pub tickets: BTreeSet<CompileTicket>,
    /// The modules compiled, or why the compile failed.
    pub outcome: Result<usize, String>,
}

/// Never coalesces: each compile answers its own tickets.
impl Supersedes for CompileDone {}

impl Staged for CompileDone {}
