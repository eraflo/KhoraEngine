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

//! Expressions — the nodes that produce a value.

use super::types::TypeRef;
use crate::diagnostics::Span;

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `42`
    Int {
        /// The value.
        value: i64,
        /// Its span.
        span: Span,
    },
    /// `2.0`
    Float {
        /// The value.
        value: f32,
        /// Its span.
        span: Span,
    },
    /// `"text"`
    Str {
        /// The contents, escapes already resolved.
        value: String,
        /// Its span.
        span: Span,
    },
    /// `2s`, in seconds.
    Duration {
        /// Seconds.
        value: f32,
        /// Its span.
        span: Span,
    },
    /// `90deg`, in radians.
    Angle {
        /// Radians.
        value: f32,
        /// Its span.
        span: Span,
    },
    /// `true` / `false`
    Bool {
        /// The value.
        value: bool,
        /// Its span.
        span: Span,
    },
    /// `null`
    Null(Span),
    /// `this`
    This(Span),
    /// A name.
    Ident {
        /// The name.
        name: String,
        /// Its span.
        span: Span,
    },
    /// `[1, 2, 3]`
    ArrayLit {
        /// The elements.
        elements: Vec<Expr>,
        /// The whole literal.
        span: Span,
    },
    /// `a + b`
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `-a`, `!a`
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `target = value`, including the compound forms.
    Assign {
        /// What is written to.
        target: Box<Expr>,
        /// `None` for `=`, otherwise the arithmetic part of `+=` and friends.
        op: Option<BinaryOp>,
        /// The value.
        value: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `f(a, b)`
    Call {
        /// What is called.
        callee: Box<Expr>,
        /// Arguments.
        args: Vec<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `a.b`
    Field {
        /// The receiver.
        object: Box<Expr>,
        /// The field name.
        name: String,
        /// The whole expression.
        span: Span,
    },
    /// `a?.b`
    OptionalField {
        /// The receiver.
        object: Box<Expr>,
        /// The field name.
        name: String,
        /// The whole expression.
        span: Span,
    },
    /// `a[i]`
    Index {
        /// The collection.
        object: Box<Expr>,
        /// The index.
        index: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `c ? a : b`
    Ternary {
        /// Condition.
        condition: Box<Expr>,
        /// Value when true.
        then_value: Box<Expr>,
        /// Value when false.
        else_value: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `new Health(1, 2)`
    New {
        /// The type constructed.
        ty: TypeRef,
        /// Constructor arguments.
        args: Vec<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `var name = expr`, used as a condition.
    ///
    /// The narrowing form: it binds, tests the value for presence, and gives
    /// the branch a non-optional `name`. A distinct node rather than an
    /// `Assign` because the two mean different things — one introduces a name
    /// and tests it, the other overwrites an existing one — and conflating them
    /// would make the checker guess which was intended.
    Binding {
        /// The name introduced, non-optional inside the branch.
        name: String,
        /// The optional being tested.
        value: Box<Expr>,
        /// The whole binding.
        span: Span,
    },
    /// `await expr`
    ///
    /// Only legal inside an `async` member — enforced by the type checker, not
    /// the parser, so the error can name the enclosing member.
    Await {
        /// What is awaited: a duration, an event, or a handle.
        operand: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
    /// `(float)x`
    Cast {
        /// Target type.
        ty: TypeRef,
        /// The value.
        operand: Box<Expr>,
        /// The whole expression.
        span: Span,
    },
}

impl Expr {
    /// Where the expression was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Int { span, .. }
            | Self::Float { span, .. }
            | Self::Str { span, .. }
            | Self::Duration { span, .. }
            | Self::Angle { span, .. }
            | Self::Bool { span, .. }
            | Self::Ident { span, .. }
            | Self::ArrayLit { span, .. }
            | Self::Binary { span, .. }
            | Self::Unary { span, .. }
            | Self::Assign { span, .. }
            | Self::Call { span, .. }
            | Self::Field { span, .. }
            | Self::OptionalField { span, .. }
            | Self::Index { span, .. }
            | Self::Ternary { span, .. }
            | Self::New { span, .. }
            | Self::Binding { span, .. }
            | Self::Await { span, .. }
            | Self::Cast { span, .. } => *span,
            Self::Null(span) | Self::This(span) => *span,
        }
    }
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `==`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Less,
    /// `<=`
    LessEq,
    /// `>`
    Greater,
    /// `>=`
    GreaterEq,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `??`
    Coalesce,
}

impl BinaryOp {
    /// The overloadable form, when there is one.
    ///
    /// `&&`, `||` and `??` return `None` — they are not overloadable, and this
    /// is where that rule lives rather than being restated at each use.
    pub fn overloadable(self) -> Option<super::decl::OverloadableOp> {
        use super::decl::OverloadableOp;
        Some(match self {
            Self::Add => OverloadableOp::Add,
            Self::Sub => OverloadableOp::Sub,
            Self::Mul => OverloadableOp::Mul,
            Self::Div => OverloadableOp::Div,
            Self::Rem => OverloadableOp::Rem,
            Self::Eq => OverloadableOp::Eq,
            Self::NotEq => OverloadableOp::NotEq,
            Self::Less => OverloadableOp::Less,
            Self::LessEq => OverloadableOp::LessEq,
            Self::Greater => OverloadableOp::Greater,
            Self::GreaterEq => OverloadableOp::GreaterEq,
            Self::And | Self::Or | Self::Coalesce => return None,
        })
    }
}

/// A prefix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `!`
    Not,
}
