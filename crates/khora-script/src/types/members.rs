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

//! A behavior's members: its fields, methods, handlers, schedules and states.
//!
//! Scope follows the declaration. A behavior's fields and methods are visible
//! to every member, inside its states too; a state's own data and methods only
//! inside that state, where its methods come first — ahead of the behavior's,
//! then the free functions and the engine's. A state's method called from
//! anywhere else is refused, naming the state it belongs to.

use super::{collect_state_names, Checker, Context, FnInfo, Scopes, Ty};
use crate::ast::{BehaviorDecl, BehaviorMember};

impl Checker {
    pub(super) fn check_behavior(&mut self, decl: &BehaviorDecl) {
        let states = collect_state_names(&decl.members);

        // A behavior's methods are callable from any of its members, including
        // from inside a state, so they are collected before any body is
        // checked — otherwise a method could only call one declared above it.
        // A state's own are in scope only inside it.
        let saved = self.functions.clone();
        self.state_methods.clear();
        self.method_names.clear();
        self.collect_methods(&decl.members);

        // Behavior fields are visible to every member, including inside states.
        self.scopes = Scopes::new();
        self.field_names.clear();
        self.declare_fields(&decl.members);
        self.check_first_state(&decl.members);
        self.check_members(&decl.members, &decl.name, &states);

        // Methods belong to their behavior; leaving them in scope would let the
        // next declaration call them.
        self.functions = saved;
        self.state_methods.clear();
        self.field_names.clear();
        self.method_names.clear();
    }

    /// Brings the methods declared directly in `members` into scope, over any
    /// free function or outer method of the same name, and records which state
    /// declares each method of the states among them.
    fn collect_methods(&mut self, members: &[BehaviorMember]) {
        for member in members {
            match member {
                BehaviorMember::Method(method) => {
                    self.refuse_reserved(&method.name, method.name_span);
                    self.method_names.insert(method.name.clone());
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
                BehaviorMember::State(state) => {
                    for inner in &state.members {
                        if let BehaviorMember::Method(method) = inner {
                            self.state_methods
                                .entry(method.name.clone())
                                .or_default()
                                .push(state.name.clone());
                        }
                    }
                }
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
                self.field_names.push(field.name.clone());
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

    /// Refuses parameters on the first state: every instance starts in it,
    /// entered by no `become`, so nothing could give them a value.
    fn check_first_state(&mut self, members: &[BehaviorMember]) {
        let first = members.iter().find_map(|member| match member {
            BehaviorMember::State(state) => Some(state),
            _ => None,
        });
        if let Some(state) = first.filter(|state| !state.params.is_empty()) {
            self.error_note(
                format!("`{}` is the first state, so it cannot take parameters", state.name),
                state.name_span,
                "every instance starts in the first state declared, entered with no arguments — declare another state first",
            );
        }
    }

    fn check_members(&mut self, members: &[BehaviorMember], owner: &str, states: &[String]) {
        for (position, member) in members.iter().enumerate() {
            match member {
                BehaviorMember::Field(field) => {
                    if let Some(default) = &field.default {
                        let declared = self.resolve(&field.ty);
                        let mut context = Context::sync(owner, Ty::Void);
                        context.owner = Some(owner.to_owned());
                        // This field and those below it have no value yet.
                        let unready = members[position..]
                            .iter()
                            .filter_map(|member| match member {
                                BehaviorMember::Field(field) => Some(field.name.clone()),
                                _ => None,
                            })
                            .collect();
                        let ready = std::mem::replace(&mut self.unready, unready);
                        let actual = self.check_expr(default, &context);
                        self.unready = ready;
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
                        returns: returns.clone(),
                        in_loop: false,
                        states: states.to_vec(),
                        owner: Some(owner.to_owned()),
                    };
                    self.check_block(&method.body, &context);
                    self.check_returns(&method.name, &returns, &method.body, method.name_span);
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
                    let fields = self.field_names.len();
                    for param in &state.params {
                        let ty = self.resolve(&param.ty);
                        self.scopes.declare(&param.name, ty, param.span);
                        self.field_names.push(param.name.clone());
                    }
                    self.declare_fields(&state.members);
                    // Its methods too: inside the state they come first, ahead
                    // of the behavior's and the free functions.
                    let outer = self.functions.clone();
                    let outer_methods = self.method_names.clone();
                    self.collect_methods(&state.members);
                    self.check_members(&state.members, owner, states);
                    self.functions = outer;
                    self.method_names = outer_methods;
                    self.field_names.truncate(fields);
                    self.scopes.pop();
                }
            }
        }
    }
}
