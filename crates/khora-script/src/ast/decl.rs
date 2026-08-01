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

//! Declarations — the nodes that introduce a name.

use super::expr::Expr;
use super::stmt::Block;
use super::types::TypeRef;
use crate::diagnostics::Span;

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

/// `struct Loot { string name; int value; }`
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

impl BehaviorMember {
    /// Where the member was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Field(decl) => decl.span,
            Self::State(decl) => decl.span,
            Self::Method(decl) => decl.span,
            Self::Handler(decl) => decl.span,
            Self::Every(decl) => decl.span,
            Self::After(decl) => decl.span,
        }
    }
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
