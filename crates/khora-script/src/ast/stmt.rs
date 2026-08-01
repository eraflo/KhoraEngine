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

//! Statements — the nodes that happen in order.

use super::expr::Expr;
use super::types::TypeRef;
use crate::diagnostics::Span;

/// `{ … }`
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Statements in order.
    pub statements: Vec<Stmt>,
    /// The braces and everything between.
    pub span: Span,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `int x = 1;` or `var x = 1;` — `var` leaves `ty` as `None`.
    Let {
        /// Declared type, or `None` for `var`.
        ty: Option<TypeRef>,
        /// Variable name.
        name: String,
        /// Initialiser. Required for `var`, since there is nothing else to
        /// infer from.
        value: Option<Expr>,
        /// The whole statement.
        span: Span,
    },
    /// An expression evaluated for effect.
    Expr(Expr),
    /// `if (c) { … } else { … }`
    If {
        /// Condition.
        condition: Expr,
        /// Taken when true.
        then_branch: Block,
        /// Taken otherwise.
        else_branch: Option<Box<Stmt>>,
        /// The whole statement.
        span: Span,
    },
    /// `while (c) { … }`
    While {
        /// Condition.
        condition: Expr,
        /// Body.
        body: Block,
        /// The whole statement.
        span: Span,
    },
    /// `for (init; condition; step) { … }`
    For {
        /// Runs once before the loop.
        init: Option<Box<Stmt>>,
        /// Checked before each iteration.
        condition: Option<Expr>,
        /// Runs after each iteration.
        step: Option<Expr>,
        /// Body.
        body: Block,
        /// The whole statement.
        span: Span,
    },
    /// `foreach (var x in xs) { … }`
    Foreach {
        /// Element type, or `None` for `var`.
        ty: Option<TypeRef>,
        /// Loop variable.
        name: String,
        /// What is iterated.
        iterable: Expr,
        /// Body.
        body: Block,
        /// The whole statement.
        span: Span,
    },
    /// `return;` or `return expr;`
    Return {
        /// The returned value, if any.
        value: Option<Expr>,
        /// The whole statement.
        span: Span,
    },
    /// `break;`
    Break(Span),
    /// `continue;`
    Continue(Span),
    /// `become Chase(enemy);`
    ///
    /// A statement, not an expression: a transition ends the current state's
    /// turn, and letting it appear inside an expression would beg the question
    /// of what the rest of that expression means.
    Become {
        /// Target state.
        state: String,
        /// Arguments for the state's parameters.
        args: Vec<Expr>,
        /// The whole statement.
        span: Span,
    },
    /// `match (x) { … }`
    Match {
        /// The scrutinee.
        subject: Expr,
        /// Arms, checked for exhaustiveness later.
        arms: Vec<MatchArm>,
        /// The whole statement.
        span: Span,
    },
    /// A bare `{ … }`.
    Block(Block),
}

impl Stmt {
    /// Where the statement was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. }
            | Self::If { span, .. }
            | Self::While { span, .. }
            | Self::For { span, .. }
            | Self::Foreach { span, .. }
            | Self::Return { span, .. }
            | Self::Become { span, .. }
            | Self::Match { span, .. } => *span,
            Self::Break(span) | Self::Continue(span) => *span,
            Self::Expr(expr) => expr.span(),
            Self::Block(block) => block.span,
        }
    }
}

/// One arm of a `match`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// What it matches.
    pub pattern: Pattern,
    /// What it runs.
    pub body: Block,
    /// The whole arm.
    pub span: Span,
}

/// A `match` pattern. Deliberately narrow: optionals and states are what
/// gameplay actually branches on.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `null`
    Null(Span),
    /// `Entity e` — matches a present optional and binds it.
    Binding {
        /// The type to match.
        ty: TypeRef,
        /// The name bound in the arm.
        name: String,
        /// The whole pattern.
        span: Span,
    },
    /// `_`
    Wildcard(Span),
}

impl Pattern {
    /// Where the pattern was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Null(span) | Self::Wildcard(span) => *span,
            Self::Binding { span, .. } => *span,
        }
    }
}
