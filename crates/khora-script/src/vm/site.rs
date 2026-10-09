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

//! A named place in a function where a frame can stand.
//!
//! A suspended frame stands at one of these: a function's entry, a
//! statement's start, a loop's head, just after an `await`, or just after a
//! call returns. The name comes from the source's structure — what a statement
//! says, not where it sits — so an edit elsewhere in the function leaves it
//! unchanged, and a frame frozen at it can be found again in the edited code.

use serde::{Deserialize, Serialize};

use super::instruction::Reg;

/// One place a frame can stand, and what is live there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    /// Its name, unique within the function.
    pub name: String,
    /// Where a frame standing here resumes.
    pub pc: u32,
    /// What kind of place it is.
    pub kind: SiteKind,
    /// Every local in scope here, parameters included.
    pub locals: Vec<SiteLocal>,
    /// The temporaries live across the site, in allocation order.
    pub temporaries: Vec<Reg>,
}

/// What kind of place a [`Site`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteKind {
    /// The function's entry.
    Entry,
    /// A statement's start.
    Statement,
    /// A loop's head: the target of its back edge.
    LoopHead,
    /// Just after an `await`.
    Await,
    /// Just before an operation whose cost depends on what it touches — a
    /// copy, a field read or write of an array. A run that cannot pay for it
    /// stops here, with the operation not yet done.
    Checkpoint,
    /// Just after a call returns, in the caller.
    Return {
        /// Where the callee's frame begins, relative to the caller's.
        base: Reg,
        /// Where the callee's result lands, relative to the caller's.
        dst: Reg,
    },
}

/// A local in scope at a [`Site`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteLocal {
    /// Its name.
    pub name: String,
    /// Its type, as the checker prints it.
    pub ty: String,
    /// The block that declares it: the path of its enclosing statements and
    /// branches, `param` for a parameter. With the name, what tells a local
    /// apart from one it shadows or that shadows it — an edit may remove
    /// either.
    #[serde(default)]
    pub scope: String,
    /// The register holding it.
    pub register: Reg,
}
