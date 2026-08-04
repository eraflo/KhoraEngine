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

//! Type checking.
//!
//! Where the guarantees the language was designed around are actually made:
//!
//! | Rule | Enforced by |
//! |---|---|
//! | A `T?` cannot be used before it is tested | [`expr`], via narrowing in [`scope`] |
//! | `Duration` and `Angle` never decay to `float` | [`ty::Ty::accepts`] and the arithmetic rules in [`expr`] |
//! | `await` only inside an `async` member | the [`Context`] carried through [`stmt`] |
//! | An overloaded operator is resolved statically | [`expr`], against the struct's declarations |
//!
//! Two passes. The first collects declarations, so a behavior may refer to a
//! struct declared below it — order in a file should not be load-bearing. The
//! second checks bodies against what the first learned.
//!
//! Like the parser, the checker does not stop at the first problem: an
//! expression that fails gets [`ty::Ty::Error`], which is compatible with
//! everything, so one mistake yields one message instead of a cascade.

pub mod expr;
pub mod scope;
pub mod stmt;
pub mod ty;

#[cfg(test)]
mod tests;

pub use scope::Scopes;
pub use ty::Ty;

use std::collections::HashMap;

use crate::ast::{
    BehaviorDecl, BehaviorMember, FunctionDecl, Item, Module, OperatorDecl, OverloadableOp,
    StructDecl,
};
use crate::diagnostics::{Diagnostic, Severity, Span};

/// What a check produced.
#[derive(Debug, Clone)]
pub struct Checked {
    /// Problems found. Empty means the module type-checks.
    pub diagnostics: Vec<Diagnostic>,
}

impl Checked {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Type-checks a parsed module.
pub fn check(module: &Module) -> Checked {
    check_with(module, &crate::native::NativeRegistry::discovered())
}

/// Checks a module against a specific set of engine functions.
///
/// The registry is a parameter because it is part of what "well-typed" means: a
/// program calling `Raycast` is correct against a host that exposes it and
/// wrong against one that does not, and the compiler must be told which. Passing
/// the same registry here and to [`compile`](crate::bytecode::compile) is what
/// keeps a call's index meaning the same thing on both sides.
///
/// [`compile`]: crate::bytecode::compile
pub fn check_with(module: &Module, natives: &crate::native::NativeRegistry) -> Checked {
    let mut checker = Checker::new();
    checker.collect_natives(natives);
    checker.collect(module);
    checker.check_bodies(module);
    Checked {
        diagnostics: checker.diagnostics,
    }
}

/// What a struct declares, gathered in the first pass.
#[derive(Debug, Clone)]
pub struct StructInfo {
    /// Field types by name.
    pub fields: HashMap<String, Ty>,
    /// Operator overloads, keyed by operator and right-hand type name.
    ///
    /// Keyed on the *written* right-hand type rather than the resolved one so
    /// collection stays a single pass; resolution happens when a use is
    /// checked.
    pub operators: Vec<OperatorInfo>,
}

/// One resolved operator overload.
#[derive(Debug, Clone)]
pub struct OperatorInfo {
    /// Which operator.
    pub op: OverloadableOp,
    /// Operand types.
    pub params: Vec<Ty>,
    /// Result type.
    pub result: Ty,
}

/// A callable's signature.
#[derive(Debug, Clone)]
pub struct FnInfo {
    /// Parameter types.
    ///
    /// The exact list, unless [`variadic`](Self::variadic) — then the minimum.
    pub params: Vec<Ty>,
    /// Return type.
    pub result: Ty,
    /// Whether extra arguments are allowed.
    ///
    /// Only an engine function can be: a script's own always declares exactly
    /// what it takes. See [`NativeFn::variadic`](crate::native::NativeFn::variadic)
    /// for why one would be.
    pub variadic: bool,
}

/// Where the checker currently is, which decides what is legal.
///
/// `await` is the reason this exists: it is well-formed syntax anywhere, but
/// only *means* something inside a member that can suspend. Carrying the
/// context lets the error name the member instead of just refusing.
#[derive(Debug, Clone)]
pub struct Context {
    /// Whether `await` is allowed here.
    pub allows_await: bool,
    /// Name of the enclosing member, for diagnostics.
    pub member: String,
    /// What `return` must produce.
    pub returns: Ty,
    /// Whether `break` and `continue` have a target.
    pub in_loop: bool,
    /// States reachable by `become`, empty outside a behavior.
    pub states: Vec<String>,
    /// The behavior this member belongs to, or `None` in a free function.
    ///
    /// What decides whether `this` means anything: a behavior is a component on
    /// an entity, so its members have a subject and a free function does not.
    pub owner: Option<String>,
}

impl Context {
    /// A context for a member that cannot suspend.
    pub fn sync(member: impl Into<String>, returns: Ty) -> Self {
        Self {
            allows_await: false,
            member: member.into(),
            returns,
            in_loop: false,
            states: Vec::new(),
            owner: None,
        }
    }
}

/// The checker's state across both passes.
pub struct Checker {
    /// Structs by name.
    pub structs: HashMap<String, StructInfo>,
    /// Free functions by name.
    pub functions: HashMap<String, FnInfo>,
    /// Engine functions by name.
    ///
    /// Kept apart from [`functions`](Self::functions) rather than merged into
    /// it for two reasons: the compiler emits a different instruction for each,
    /// and a script declaring a function that shadows an engine one deserves to
    /// be told which name it collided with rather than "declared twice".
    pub natives: HashMap<String, FnInfo>,
    /// Behavior names, so `this` and `become` can be validated.
    pub behaviors: Vec<String>,
    /// Accumulated problems.
    pub diagnostics: Vec<Diagnostic>,
    /// Lexical scopes for the body being checked.
    pub scopes: Scopes,
}

impl Checker {
    fn new() -> Self {
        Self {
            structs: HashMap::new(),
            functions: HashMap::new(),
            natives: HashMap::new(),
            behaviors: Vec::new(),
            diagnostics: Vec::new(),
            scopes: Scopes::new(),
        }
    }

    /// Records an error.
    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    /// Records an error carrying the reasoning behind the rule.
    pub fn error_note(&mut self, message: impl Into<String>, span: Span, note: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(message, span).with_note(note));
    }

    /// Resolves a written type, reporting and yielding [`Ty::Error`] when the
    /// name is unknown.
    pub fn resolve(&mut self, ty: &crate::ast::TypeRef) -> Ty {
        let known: Vec<String> = self.structs.keys().cloned().collect();
        match ty::resolve(ty, &|name| known.iter().any(|s| s == name)) {
            Some(resolved) => resolved,
            None => {
                self.error_note(
                    format!("unknown type `{}`", written_name(ty)),
                    ty.span(),
                    "declare it as a `struct`, or use a built-in: int, float, bool, string, Duration, Angle, Entity",
                );
                Ty::Error
            }
        }
    }

    // ── Pass one: declarations ────────────────────────

    /// Gathers every declared name before any body is checked, so a file's
    /// order does not decide what compiles.
    fn collect(&mut self, module: &Module) {
        // Struct names first, so a struct may hold another declared later.
        for item in &module.items {
            if let Item::Struct(decl) = item {
                self.structs.insert(
                    decl.name.clone(),
                    StructInfo {
                        fields: HashMap::new(),
                        operators: Vec::new(),
                    },
                );
            }
            if let Item::Behavior(decl) = item {
                self.behaviors.push(decl.name.clone());
            }
        }

        self.report_duplicate_names(module);

        // Now the members, with every struct name already known.
        for item in &module.items {
            match item {
                Item::Struct(decl) => self.collect_struct(decl),
                Item::Function(decl) => self.collect_function(decl),
                Item::Behavior(_) => {}
            }
        }
    }

    fn report_duplicate_names(&mut self, module: &Module) {
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for item in &module.items {
            if let Some(first) = seen.get(item.name()) {
                let first = *first;
                let name = item.name().to_owned();
                self.error_note(
                    format!("`{name}` is declared twice"),
                    item.name_span(),
                    format!("first declared at byte {}", first.start),
                );
            } else {
                seen.insert(item.name(), item.name_span());
            }
        }
    }

    fn collect_struct(&mut self, decl: &StructDecl) {
        let mut fields = HashMap::new();
        for field in &decl.fields {
            let ty = self.resolve(&field.ty);
            if fields.contains_key(&field.name) {
                self.error(
                    format!("field `{}` is declared twice", field.name),
                    field.span,
                );
                continue;
            }
            fields.insert(field.name.clone(), ty);
        }

        let operators = decl
            .operators
            .iter()
            .map(|op| self.collect_operator(op))
            .collect();

        if let Some(info) = self.structs.get_mut(&decl.name) {
            info.fields = fields;
            info.operators = operators;
        }
    }

    fn collect_operator(&mut self, decl: &OperatorDecl) -> OperatorInfo {
        let params: Vec<Ty> = decl.params.iter().map(|p| self.resolve(&p.ty)).collect();
        let result = self.resolve(&decl.return_ty);

        // Binary operators take two operands; unary `-` takes one. Anything
        // else has no call shape the language could produce.
        let expected: &[usize] = if decl.op == OverloadableOp::Sub {
            &[1, 2]
        } else {
            &[2]
        };
        if !expected.contains(&params.len()) {
            self.error_note(
                format!(
                    "an overload of this operator takes {} operands, found {}",
                    expected
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(" or "),
                    params.len()
                ),
                decl.span,
                "binary operators take two; only `-` may also be unary",
            );
        }

        OperatorInfo {
            op: decl.op,
            params,
            result,
        }
    }

    /// Records what the host exposes, before any declaration is read.
    fn collect_natives(&mut self, natives: &crate::native::NativeRegistry) {
        for native in natives.iter() {
            self.natives.insert(
                native.name.to_owned(),
                FnInfo {
                    params: native.params.iter().map(|p| p.to_ty()).collect(),
                    result: native.result.to_ty(),
                    variadic: native.variadic,
                },
            );
        }
    }

    fn collect_function(&mut self, decl: &FunctionDecl) {
        let params = decl.params.iter().map(|p| self.resolve(&p.ty)).collect();
        let result = self.resolve(&decl.return_ty);
        if self.functions.contains_key(&decl.name) {
            self.error(
                format!("function `{}` is declared twice", decl.name),
                decl.name_span,
            );
            return;
        }
        if self.natives.contains_key(&decl.name) {
            // Shadowing would compile — the checker would simply prefer the
            // script's — and that is exactly the problem: every existing call
            // to the engine function would quietly start meaning something
            // else.
            self.error_note(
                format!("`{}` is already an engine function", decl.name),
                decl.name_span,
                "rename this one; a script cannot replace what the engine exposes",
            );
            return;
        }
        self.functions.insert(
            decl.name.clone(),
            FnInfo {
                params,
                result,
                variadic: false,
            },
        );
    }

    // ── Pass two: bodies ──────────────────────────────

    fn check_bodies(&mut self, module: &Module) {
        for item in &module.items {
            match item {
                Item::Function(decl) => self.check_function(decl),
                Item::Behavior(decl) => self.check_behavior(decl),
                Item::Struct(decl) => self.check_struct_bodies(decl),
            }
        }
    }

    fn check_struct_bodies(&mut self, decl: &StructDecl) {
        for operator in &decl.operators {
            self.scopes = Scopes::new();
            for param in &operator.params {
                let ty = self.resolve(&param.ty);
                self.scopes.declare(&param.name, ty, param.span);
            }
            let returns = self.resolve(&operator.return_ty);
            let context = Context::sync(format!("operator on `{}`", decl.name), returns);
            self.check_block(&operator.body, &context);
        }
    }

    fn check_function(&mut self, decl: &FunctionDecl) {
        self.scopes = Scopes::new();
        for param in &decl.params {
            let ty = self.resolve(&param.ty);
            self.scopes.declare(&param.name, ty, param.span);
        }
        let returns = self.resolve(&decl.return_ty);
        let context = Context::sync(decl.name.clone(), returns);
        self.check_block(&decl.body, &context);
    }

    fn check_behavior(&mut self, decl: &BehaviorDecl) {
        let states = collect_state_names(&decl.members);

        // A behavior's methods are callable from any of its members, including
        // from inside a state, so they are collected before any body is
        // checked — otherwise a method could only call one declared above it.
        let saved = self.functions.clone();
        self.collect_methods(&decl.members);

        // Behavior fields are visible to every member, including inside states.
        self.scopes = Scopes::new();
        self.declare_fields(&decl.members);
        self.check_members(&decl.members, &decl.name, &states);

        // Methods belong to their behavior; leaving them in scope would let the
        // next declaration call them.
        self.functions = saved;
    }

    /// Registers every method declared in `members`, recursing into states.
    fn collect_methods(&mut self, members: &[BehaviorMember]) {
        for member in members {
            match member {
                BehaviorMember::Method(method) => {
                    let params = method.params.iter().map(|p| self.resolve(&p.ty)).collect();
                    let result = self.resolve(&method.return_ty);
                    self.functions.insert(
                        method.name.clone(),
                        FnInfo {
                            params,
                            result,
                            variadic: false,
                        },
                    );
                }
                BehaviorMember::State(state) => self.collect_methods(&state.members),
                _ => {}
            }
        }
    }

    /// Holds an engine-invoked member to the shape the engine will call it with.
    ///
    /// The mistake this exists for is `void Update()` where `void Update(float
    /// dt)` was meant. Dispatch is by name, so the wrong signature does not
    /// produce a call that fails — it produces a body that never runs, and an
    /// author left watching nothing happen has nothing to search for.
    fn check_lifecycle(&mut self, method: &crate::ast::MethodDecl) {
        let Some(hook) = crate::lifecycle::of(&method.name) else {
            return;
        };

        if !hook.called {
            self.diagnostics
                .push(crate::diagnostics::Diagnostic::warning(
                    format!(
                        "`{}` is reserved but the engine does not call it yet",
                        hook.name
                    ),
                    method.name_span,
                ));
        }

        let returns = self.resolve(&method.return_ty);
        if returns != Ty::Void {
            self.error_note(
                format!("`{}` must return nothing", hook.name),
                method.name_span,
                "the engine calls it and has nowhere to put a result",
            );
        }

        let found: Vec<Ty> = method.params.iter().map(|p| self.resolve(&p.ty)).collect();
        if found != hook.params {
            let expected = hook
                .params
                .iter()
                .map(Ty::name)
                .collect::<Vec<_>>()
                .join(", ");
            self.error_note(
                format!("`{}` must take ({expected})", hook.name),
                method.name_span,
                "the engine calls this member itself, so its shape is fixed — a \
                 different one is not an error at the call site, it is a body that \
                 never runs",
            );
        }
    }

    fn declare_fields(&mut self, members: &[BehaviorMember]) {
        for member in members {
            if let BehaviorMember::Field(field) = member {
                let ty = self.resolve(&field.ty);
                if let Some(previous) = self.scopes.declare(&field.name, ty, field.span) {
                    let _ = previous;
                    self.error(
                        format!("field `{}` is declared twice", field.name),
                        field.span,
                    );
                }
            }
        }
    }

    fn check_members(&mut self, members: &[BehaviorMember], owner: &str, states: &[String]) {
        for member in members {
            match member {
                BehaviorMember::Field(field) => {
                    if let Some(default) = &field.default {
                        let declared = self.resolve(&field.ty);
                        let mut context = Context::sync(owner, Ty::Void);
                        context.owner = Some(owner.to_owned());
                        let actual = self.check_expr(default, &context);
                        self.expect_assignable(&declared, &actual, default.span());
                    }
                }
                BehaviorMember::Method(method) => {
                    self.check_lifecycle(method);
                    self.scopes.push();
                    for param in &method.params {
                        let ty = self.resolve(&param.ty);
                        self.scopes.declare(&param.name, ty, param.span);
                    }
                    let returns = self.resolve(&method.return_ty);
                    let context = Context {
                        allows_await: method.is_async,
                        member: method.name.clone(),
                        returns,
                        in_loop: false,
                        states: states.to_vec(),
                        owner: Some(owner.to_owned()),
                    };
                    self.check_block(&method.body, &context);
                    self.scopes.pop();
                }
                BehaviorMember::Handler(handler) => {
                    self.scopes.push();
                    for param in &handler.params {
                        let ty = self.resolve(&param.ty);
                        self.scopes.declare(&param.name, ty, param.span);
                    }
                    let context = Context {
                        allows_await: false,
                        member: format!("on {}", handler.event),
                        returns: Ty::Void,
                        in_loop: false,
                        states: states.to_vec(),
                        owner: Some(owner.to_owned()),
                    };
                    self.check_block(&handler.body, &context);
                    self.scopes.pop();
                }
                BehaviorMember::Every(every) => {
                    let context = Context {
                        allows_await: false,
                        member: "every".to_owned(),
                        returns: Ty::Void,
                        in_loop: false,
                        states: states.to_vec(),
                        owner: Some(owner.to_owned()),
                    };
                    let interval = self.check_expr(&every.interval, &context);
                    self.expect_duration(&interval, every.interval.span(), "every");
                    self.check_block(&every.body, &context);
                }
                BehaviorMember::After(after) => {
                    let context = Context {
                        allows_await: false,
                        member: "after".to_owned(),
                        returns: Ty::Void,
                        in_loop: false,
                        states: states.to_vec(),
                        owner: Some(owner.to_owned()),
                    };
                    let delay = self.check_expr(&after.delay, &context);
                    self.expect_duration(&delay, after.delay.span(), "after");
                    self.check_block(&after.body, &context);
                }
                BehaviorMember::State(state) => {
                    // A state's own fields and parameters are visible only
                    // inside it — that containment is the point of `state`.
                    self.scopes.push();
                    for param in &state.params {
                        let ty = self.resolve(&param.ty);
                        self.scopes.declare(&param.name, ty, param.span);
                    }
                    self.declare_fields(&state.members);
                    self.check_members(&state.members, owner, states);
                    self.scopes.pop();
                }
            }
        }
    }

    /// Reports when `actual` cannot be used where `expected` is wanted.
    pub fn expect_assignable(&mut self, expected: &Ty, actual: &Ty, span: Span) {
        if expected.accepts(actual) {
            return;
        }

        // The two mistakes worth explaining rather than merely stating.
        if actual.is_optional() && expected.accepts(&actual.unwrapped()) {
            self.error_note(
                format!(
                    "expected `{}`, found `{}`",
                    expected.name(),
                    actual.name()
                ),
                span,
                "test it first — `if (var x = value)` narrows it for the branch, and `match` binds it per arm",
            );
            return;
        }
        if expected.is_unit() || actual.is_unit() {
            self.error_note(
                format!(
                    "expected `{}`, found `{}`",
                    expected.name(),
                    actual.name()
                ),
                span,
                "Duration and Angle are distinct types, not floats — convert explicitly with `.Seconds` or `.Radians`",
            );
            return;
        }

        self.error(
            format!("expected `{}`, found `{}`", expected.name(), actual.name()),
            span,
        );
    }

    fn expect_duration(&mut self, actual: &Ty, span: Span, what: &str) {
        if matches!(actual, Ty::Duration | Ty::Error) {
            return;
        }
        self.error_note(
            format!("`{what}` needs a Duration, found `{}`", actual.name()),
            span,
            "write a duration literal such as `0.5s`, `250ms` or `2min`",
        );
    }
}

/// The names of the states declared directly in `members`.
fn collect_state_names(members: &[BehaviorMember]) -> Vec<String> {
    members
        .iter()
        .filter_map(|member| match member {
            BehaviorMember::State(state) => Some(state.name.clone()),
            _ => None,
        })
        .collect()
}

/// A written type, spelled back for a message.
fn written_name(ty: &crate::ast::TypeRef) -> String {
    use crate::ast::TypeRef;
    match ty {
        TypeRef::Void => "void".to_owned(),
        TypeRef::Named { name, .. } => name.clone(),
        TypeRef::Optional { inner, .. } => format!("{}?", written_name(inner)),
        TypeRef::Array { element, .. } => format!("{}[]", written_name(element)),
        TypeRef::Generic { name, args, .. } => format!(
            "{name}<{}>",
            args.iter().map(written_name).collect::<Vec<_>>().join(", ")
        ),
    }
}
