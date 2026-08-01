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
//! **Everything carries a [`Span`].** Type errors, nullability errors and the
//! `await`-outside-a-sequence check all report against source the author wrote,
//! and a node that lost its position can only produce a vague message.
//!
//! **Ergon's own constructs are nodes, not desugarings.** `state`, `become`,
//! `every` and `after` could each be encoded as something more primitive —
//! `become` as an assignment, `every` as a hidden timer field. They are not,
//! because the error messages would then talk about the encoding rather than
//! what the author wrote. A language whose diagnostics leak its implementation
//! is one people learn to fight.

use crate::diagnostics::Span;

/// One parsed `.erg` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// What this file pulls in, in source order.
    pub imports: Vec<Import>,
    /// Its top-level declarations.
    pub items: Vec<Item>,
}

/// `import "combat/damage.erg";` or `import "ai/steering.erg" as Steering;`
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The quoted path, relative to the project's script root.
    pub path: String,
    /// The `as` prefix, when one is given.
    pub alias: Option<String>,
    /// Covers the whole statement, so a circular-import error can point at it.
    pub span: Span,
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

/// `struct Loot { string name; int value; static Loot operator +(…) { … } }`
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    /// Type name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Data members.
    pub fields: Vec<Field>,
    /// Operator overloads declared on this struct.
    pub operators: Vec<OperatorDecl>,
    /// The whole declaration.
    pub span: Span,
}

/// A named, typed slot: a struct field or a behavior field.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Declared type.
    pub ty: TypeRef,
    /// Field name.
    pub name: String,
    /// `= expr`, when given.
    pub default: Option<Expr>,
    /// The whole declaration.
    pub span: Span,
}

/// `static Health operator -(Health h, int damage) => …;`
///
/// Overloading is resolved statically, so it costs nothing at run time — the
/// reason it is allowed here and would not be in a dynamic language.
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorDecl {
    /// Which operator is being defined.
    pub op: OverloadableOp,
    /// Where the operator token sits, for "cannot overload" errors.
    pub op_span: Span,
    /// Result type.
    pub return_ty: TypeRef,
    /// Operands. Two for binary, one for unary negation.
    pub params: Vec<Param>,
    /// The body.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// The operators a struct may define.
///
/// `&&`, `||` and `!` are absent by design: short-circuiting must not be
/// redefinable. `??` and `?.` are absent too — the meaning of null is not
/// negotiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverloadableOp {
    /// `+`
    Add,
    /// `-`, binary or unary depending on the parameter count.
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
}

/// `fn float Distance(Vec3 a, Vec3 b) { … }`
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    /// Function name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Return type; `void` is a [`TypeRef::Void`].
    pub return_ty: TypeRef,
    /// Parameters.
    pub params: Vec<Param>,
    /// Body.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// One parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Declared type.
    pub ty: TypeRef,
    /// Parameter name.
    pub name: String,
    /// The whole parameter.
    pub span: Span,
}

/// `behavior Guard { … }` — the unit attached to an entity.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDecl {
    /// `[Critical]`, `[Budget(0.2ms)]`, `[Rate(10hz)]`.
    pub attributes: Vec<Attribute>,
    /// Behavior name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Members, in source order — which is also the order they run in, so it is
    /// preserved rather than bucketed by kind.
    pub members: Vec<BehaviorMember>,
    /// The whole declaration.
    pub span: Span,
}

/// `[Critical]` or `[Budget(0.2ms)]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// Attribute name.
    pub name: String,
    /// Arguments, if any.
    pub args: Vec<Expr>,
    /// The whole attribute.
    pub span: Span,
}

/// Something declared inside a `behavior` or a `state`.
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorMember {
    /// `float speed = 3.0;`
    Field(Field),
    /// `state Patrol { … }`
    State(StateDecl),
    /// `void Update(float dt) { … }`
    Method(MethodDecl),
    /// `on Damaged(int amount) { … }`
    Handler(HandlerDecl),
    /// `every 0.5s { … }`
    Every(EveryDecl),
    /// `after 10s => become Patrol;`
    After(AfterDecl),
}

/// `state Chase(Entity prey) { … }`
///
/// A state carries its own fields and parameters, so data that only makes sense
/// while chasing cannot be read while patrolling. That containment is the point
/// — in C# the same variables sit on the class and are always in scope.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDecl {
    /// State name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Values the state is entered with, supplied by `become`.
    pub params: Vec<Param>,
    /// Its own members.
    pub members: Vec<BehaviorMember>,
    /// The whole declaration.
    pub span: Span,
}

/// `void Update(float dt) { … }` or `async void Attack() { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    /// Whether it may `await`.
    pub is_async: bool,
    /// Return type.
    pub return_ty: TypeRef,
    /// Method name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Parameters.
    pub params: Vec<Param>,
    /// Body.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// `on Damaged(int amount) { … }` or the shorthand `on Lost => become Patrol;`
#[derive(Debug, Clone, PartialEq)]
pub struct HandlerDecl {
    /// Event name.
    pub event: String,
    /// Where the event name sits.
    pub event_span: Span,
    /// Parameters the event carries.
    pub params: Vec<Param>,
    /// What runs. The arrow form is stored as a one-statement block, so
    /// everything downstream sees a single shape.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// `every 0.5s { … }`
#[derive(Debug, Clone, PartialEq)]
pub struct EveryDecl {
    /// The interval. An expression rather than a literal so a field can drive
    /// it; the type checker requires a `Duration`.
    pub interval: Expr,
    /// What runs each time.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// `after 10s => become Patrol;` or `after 2s { … }`
#[derive(Debug, Clone, PartialEq)]
pub struct AfterDecl {
    /// The delay.
    pub delay: Expr,
    /// What runs once it elapses.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

/// A written type.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    /// `void`, only valid as a return type.
    Void,
    /// `int`, `Vec3`, `Health` — resolved later, so user types and built-ins
    /// are indistinguishable here.
    Named {
        /// The name.
        name: String,
        /// Its span.
        span: Span,
    },
    /// `T?`
    Optional {
        /// The wrapped type.
        inner: Box<TypeRef>,
        /// Covers `T?`.
        span: Span,
    },
    /// `T[]`
    Array {
        /// The element type.
        element: Box<TypeRef>,
        /// Covers `T[]`.
        span: Span,
    },
    /// `Map<K, V>`
    Generic {
        /// The constructor name.
        name: String,
        /// Its arguments.
        args: Vec<TypeRef>,
        /// The whole type.
        span: Span,
    },
}

impl TypeRef {
    /// Where the type was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Void => Span::empty(0),
            Self::Named { span, .. }
            | Self::Optional { span, .. }
            | Self::Array { span, .. }
            | Self::Generic { span, .. } => *span,
        }
    }
}

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
    /// is where that rule is enforced rather than restated at each use.
    pub fn overloadable(self) -> Option<OverloadableOp> {
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
