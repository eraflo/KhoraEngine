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

//! Whether a body returns a value on every path.
//!
//! A function declared to return an `int` that can reach its end returns
//! nothing, and the caller that adds one to it faults far from the mistake. The
//! analysis is the usual one over the statement tree, conservative where a
//! condition decides:
//!
//! | Statement | Returns on every path when |
//! |---|---|
//! | `return` | always |
//! | a block | any statement in it does |
//! | `if` | it has an `else` and both branches do |
//! | `while`, `for`, `foreach` | never — the condition may be false at once |
//! | `match` | every arm does (an arm-less or partial `match` is refused already) |

use super::{Checker, Ty};
use crate::ast::{Block, Stmt};
use crate::diagnostics::Span;

/// Whether running `block` always ends at a `return`.
pub fn block_returns(block: &Block) -> bool {
    block.statements.iter().any(stmt_returns)
}

fn stmt_returns(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return { .. } => true,
        Stmt::Block(block) => block_returns(block),
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => block_returns(then_branch) && stmt_returns(else_branch),
        Stmt::Match { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|arm| block_returns(&arm.body))
        }
        _ => false,
    }
}

impl Checker {
    /// Refuses a body that can end without returning the value `returns` says
    /// it gives.
    pub(super) fn check_returns(&mut self, name: &str, returns: &Ty, body: &Block, span: Span) {
        if matches!(returns, Ty::Void | Ty::Error) || block_returns(body) {
            return;
        }
        self.error_note(
            format!("`{name}` does not return a value on every path"),
            span,
            format!(
                "a path reaches the end of `{name}` without a `return` — it would give the caller nothing where a `{}` was promised",
                returns.name()
            ),
        );
    }
}
