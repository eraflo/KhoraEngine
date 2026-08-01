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

//! Checking statements.
//!
//! This is where narrowing happens, and narrowing is what makes non-nullable
//! types pleasant rather than merely safe. `if (var enemy = SeeEnemy())` binds
//! an `Entity?`, tests it, and gives the branch an `Entity` — no unwrap, no
//! cast, and no way to reach the absent case by accident.
//!
//! The same idea drives `match`: each arm binds the subject at the type that
//! arm matched, so the null arm and the present arm cannot be confused.

use super::{Checker, Context, Ty};
use crate::ast::{Block, Expr, MatchArm, Pattern, Stmt};
use crate::diagnostics::Span;

impl Checker {
    /// Checks every statement in a block, in its own scope.
    pub fn check_block(&mut self, block: &Block, context: &Context) {
        self.scopes.push();
        for statement in &block.statements {
            self.check_stmt(statement, context);
        }
        self.scopes.pop();
    }

    /// Checks one statement.
    pub fn check_stmt(&mut self, statement: &Stmt, context: &Context) {
        match statement {
            Stmt::Let {
                ty,
                name,
                value,
                span,
            } => self.check_let(ty.as_ref(), name, value.as_ref(), *span, context),

            Stmt::Expr(expr) => {
                self.check_expr(expr, context);
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => self.check_if(condition, then_branch, else_branch.as_deref(), context),

            Stmt::While {
                condition, body, ..
            } => {
                let condition_ty = self.check_expr(condition, context);
                self.expect_bool(&condition_ty, condition.span(), "a `while` condition");
                let looping = Context {
                    in_loop: true,
                    ..context.clone()
                };
                self.check_block(body, &looping);
            }

            Stmt::For {
                init,
                condition,
                step,
                body,
                ..
            } => {
                // The initialiser's binding is visible to the condition, the
                // step and the body, so the whole loop shares one scope.
                self.scopes.push();
                if let Some(init) = init {
                    self.check_stmt(init, context);
                }
                if let Some(condition) = condition {
                    let ty = self.check_expr(condition, context);
                    self.expect_bool(&ty, condition.span(), "a `for` condition");
                }
                if let Some(step) = step {
                    self.check_expr(step, context);
                }
                let looping = Context {
                    in_loop: true,
                    ..context.clone()
                };
                self.check_block(body, &looping);
                self.scopes.pop();
            }

            Stmt::Foreach {
                ty,
                name,
                iterable,
                body,
                span,
            } => self.check_foreach(ty.as_ref(), name, iterable, body, *span, context),

            Stmt::Return { value, span } => {
                let actual = match value {
                    Some(expr) => self.check_expr(expr, context),
                    None => Ty::Void,
                };
                let reported = value.as_ref().map(|e| e.span()).unwrap_or(*span);

                if context.returns == Ty::Void && actual != Ty::Void {
                    self.error_note(
                        format!("`{}` returns nothing", context.member),
                        reported,
                        "drop the value, or give the member a return type",
                    );
                    return;
                }
                if context.returns != Ty::Void && value.is_none() {
                    self.error(
                        format!(
                            "`{}` must return a `{}`",
                            context.member,
                            context.returns.name()
                        ),
                        *span,
                    );
                    return;
                }
                self.expect_assignable(&context.returns.clone(), &actual, reported);
            }

            Stmt::Break(span) | Stmt::Continue(span) => {
                if !context.in_loop {
                    let word = if matches!(statement, Stmt::Break(_)) {
                        "break"
                    } else {
                        "continue"
                    };
                    self.error(format!("`{word}` is not inside a loop"), *span);
                }
            }

            Stmt::Become { state, args, span } => self.check_become(state, args, *span, context),

            Stmt::Match {
                subject,
                arms,
                span,
            } => self.check_match(subject, arms, *span, context),

            Stmt::Block(block) => self.check_block(block, context),
        }
    }

    fn check_let(
        &mut self,
        ty: Option<&crate::ast::TypeRef>,
        name: &str,
        value: Option<&Expr>,
        span: Span,
        context: &Context,
    ) {
        let declared = ty.map(|ty| self.resolve(ty));
        let initial = value.map(|expr| self.check_expr(expr, context));

        let bound = match (declared, initial) {
            (Some(declared), Some(actual)) => {
                let reported = value.map(|v| v.span()).unwrap_or(span);
                self.expect_assignable(&declared, &actual, reported);
                declared
            }
            (Some(declared), None) => declared,
            // `var` — the parser guarantees an initialiser, so this is the
            // inference case.
            (None, Some(actual)) => {
                if actual == Ty::Void {
                    self.error_note(
                        format!("`{name}` cannot hold nothing"),
                        span,
                        "the right-hand side produces no value",
                    );
                    Ty::Error
                } else {
                    actual
                }
            }
            (None, None) => Ty::Error,
        };

        if self.scopes.declare(name, bound, span).is_some() {
            self.error_note(
                format!("`{name}` is already declared in this scope"),
                span,
                "give it another name, or assign to the existing one",
            );
        }
    }

    fn check_if(
        &mut self,
        condition: &Expr,
        then_branch: &Block,
        else_branch: Option<&Stmt>,
        context: &Context,
    ) {
        // `if (var x = value)` is the narrowing form: bind, test for presence,
        // and see a present value inside the branch.
        if let Some((name, narrowed, span)) = self.narrowing_binding(condition, context) {
            self.scopes.push_narrowed(&name, narrowed, span);
            self.check_block(then_branch, context);
            self.scopes.pop();

            if let Some(else_branch) = else_branch {
                self.check_stmt(else_branch, context);
            }
            return;
        }

        let condition_ty = self.check_expr(condition, context);
        self.expect_bool(&condition_ty, condition.span(), "an `if` condition");
        self.check_block(then_branch, context);
        if let Some(else_branch) = else_branch {
            self.check_stmt(else_branch, context);
        }
    }

    /// Recognises `var name = expr` used as a condition, and returns the name
    /// with the type it has once known to be present.
    ///
    /// Returns `None` for anything else, so an ordinary condition follows the
    /// ordinary path.
    fn narrowing_binding(
        &mut self,
        condition: &Expr,
        context: &Context,
    ) -> Option<(String, Ty, Span)> {
        let Expr::Binding { name, value, span } = condition else {
            return None;
        };

        let value_ty = self.check_expr(value, context);
        if matches!(value_ty, Ty::Error) {
            // Already reported; bind at the error type so the branch still
            // checks and reports its own problems.
            return Some((name.clone(), Ty::Error, *span));
        }
        if !value_ty.is_optional() {
            self.error_note(
                format!("`{}` is never null", value_ty.name()),
                value.span(),
                "this form tests an optional for presence; a value that always exists needs no test",
            );
            return Some((name.clone(), value_ty, *span));
        }
        Some((name.clone(), value_ty.unwrapped(), *span))
    }

    fn check_foreach(
        &mut self,
        ty: Option<&crate::ast::TypeRef>,
        name: &str,
        iterable: &Expr,
        body: &Block,
        span: Span,
        context: &Context,
    ) {
        let sequence = self.check_expr(iterable, context);
        let element = match &sequence {
            Ty::Array(element) => (**element).clone(),
            Ty::Error => Ty::Error,
            other => {
                self.error_note(
                    format!("cannot iterate `{}`", other.name()),
                    iterable.span(),
                    "`foreach` walks an array",
                );
                Ty::Error
            }
        };

        if let Some(written) = ty {
            let declared = self.resolve(written);
            self.expect_assignable(&declared, &element, written.span());
        }

        self.scopes.push();
        self.scopes.declare(name, element, span);
        let looping = Context {
            in_loop: true,
            ..context.clone()
        };
        for statement in &body.statements {
            self.check_stmt(statement, &looping);
        }
        self.scopes.pop();
    }

    fn check_become(&mut self, state: &str, args: &[Expr], span: Span, context: &Context) {
        for argument in args {
            self.check_expr(argument, context);
        }

        if context.states.is_empty() {
            self.error_note(
                "`become` is only valid inside a behavior with states",
                span,
                "declare the target with `state Name { … }`",
            );
            return;
        }
        if !context.states.iter().any(|known| known == state) {
            self.error_note(
                format!("no state named `{state}`"),
                span,
                format!("this behavior declares: {}", context.states.join(", ")),
            );
        }
    }

    fn check_match(&mut self, subject: &Expr, arms: &[MatchArm], span: Span, context: &Context) {
        let subject_ty = self.check_expr(subject, context);

        let mut saw_null = false;
        let mut saw_binding = false;
        let mut saw_wildcard = false;

        for arm in arms {
            match &arm.pattern {
                Pattern::Null(_) => {
                    saw_null = true;
                    self.check_block(&arm.body, context);
                }
                Pattern::Wildcard(_) => {
                    saw_wildcard = true;
                    self.check_block(&arm.body, context);
                }
                Pattern::Binding {
                    ty,
                    name,
                    span: pattern_span,
                } => {
                    saw_binding = true;
                    let declared = self.resolve(ty);
                    let present = subject_ty.unwrapped();
                    if !declared.accepts(&present) && !matches!(present, Ty::Error) {
                        self.error(
                            format!(
                                "this arm matches `{}`, but the subject is `{}`",
                                declared.name(),
                                subject_ty.name()
                            ),
                            *pattern_span,
                        );
                    }
                    // The arm sees a present value, which is the whole point of
                    // matching rather than testing by hand.
                    self.scopes.push_narrowed(name, declared, *pattern_span);
                    self.check_block(&arm.body, context);
                    self.scopes.pop();
                }
            }
        }

        // Exhaustiveness, for the one shape the language matches on today.
        if subject_ty.is_optional() && !saw_wildcard && !(saw_null && saw_binding) {
            let missing = if saw_null {
                "the present case"
            } else {
                "`null`"
            };
            self.error_note(
                format!("this `match` does not cover {missing}"),
                span,
                "an optional needs both arms, or a `_` — otherwise a value would fall through unhandled",
            );
        }
    }
}
