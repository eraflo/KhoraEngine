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

//! The syntax tree.
//!
//! Shaped by two rules.
//!
//! **Everything carries a [`Span`](crate::diagnostics::Span).** Type errors,
//! nullability errors and the `await`-outside-a-sequence check all report
//! against source the author wrote, and a node that lost its position can only
//! produce a vague message.
//!
//! **Ergon's own constructs are nodes, not desugarings.** `state`, `become`,
//! `every` and `after` could each be encoded as something more primitive —
//! `become` as an assignment, `every` as a hidden timer field. They are not,
//! because the error messages would then talk about the encoding rather than
//! what the author wrote. A language whose diagnostics leak its implementation
//! is one people learn to fight.
//!
//! Split by what a node *is*: [`decl`] for things that introduce a name,
//! [`stmt`] for things that happen in order, [`expr`] for things that produce a
//! value, and [`types`] for written types.

pub mod decl;
pub mod expr;
pub mod stmt;
pub mod types;

pub use decl::{
    AfterDecl, Attribute, BehaviorDecl, BehaviorMember, EveryDecl, Field, FunctionDecl,
    HandlerDecl, Import, MethodDecl, OperatorDecl, OverloadableOp, Param, StateDecl, StructDecl,
};
pub use expr::{BinaryOp, Expr, UnaryOp};
pub use stmt::{Block, MatchArm, Pattern, Stmt};
pub use types::TypeRef;

use crate::diagnostics::Span;

/// One parsed `.erg` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// What this file pulls in, in source order.
    pub imports: Vec<Import>,
    /// Its top-level declarations.
    pub items: Vec<Item>,
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `struct Loot { … }`
    Struct(StructDecl),
    /// `fn float Distance(Vec3 a, Vec3 b) { … }`
    Function(FunctionDecl),
    /// `behavior Guard { … }`
    Behavior(BehaviorDecl),
}

impl Item {
    /// The declared name, for duplicate detection and import resolution.
    pub fn name(&self) -> &str {
        match self {
            Self::Struct(decl) => &decl.name,
            Self::Function(decl) => &decl.name,
            Self::Behavior(decl) => &decl.name,
        }
    }

    /// The name's span, so "already declared" can point at both sites.
    pub fn name_span(&self) -> Span {
        match self {
            Self::Struct(decl) => decl.name_span,
            Self::Function(decl) => decl.name_span,
            Self::Behavior(decl) => decl.name_span,
        }
    }
}
