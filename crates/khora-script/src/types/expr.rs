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

//! Inferring the type of an expression.
//!
//! Three of the language's guarantees are decided here.
//!
//! **Unit arithmetic.** `Duration` and `Angle` are closed under addition with
//! themselves and scaling by a number, and nothing else. `2s + 500ms` is a
//! Duration; `2s * 3` is a Duration; `2s * 3s` is meaningless and is rejected,
//! as is `2s + 90deg`. Ratios are the one crossing allowed: `4s / 2s` is a
//! plain number, which is what a proportion is.
//!
//! **Optionals.** Using a `T?` where a `T` is wanted fails here, and the
//! message points at the narrowing forms rather than merely restating the rule.
//!
//! **Overloads.** Resolved against the struct's declarations at check time, so
//! the call is direct and costs nothing at run time — the reason overloading is
//! affordable in a statically typed language and would not be in a dynamic one.

use super::{Checker, Context, Ty};
use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::diagnostics::Span;

impl Checker {
    /// The type of `expr`, reporting anything wrong with it.
    ///
    /// Always returns a type: a failed expression yields [`Ty::Error`], which
    /// is compatible with everything, so one mistake produces one message.
    pub fn check_expr(&mut self, expr: &Expr, context: &Context) -> Ty {
        match expr {
            Expr::Int { .. } => Ty::Int,
            Expr::Float { .. } => Ty::Float,
            Expr::Str { .. } => Ty::Str,
            Expr::Duration { .. } => Ty::Duration,
            Expr::Angle { .. } => Ty::Angle,
            Expr::Bool { .. } => Ty::Bool,

            // `null` fits any optional and nothing else. Typing it as an
            // optional of the error type gets that for free.
            Expr::Null(_) => Ty::Optional(Box::new(Ty::Error)),

            // `this` is the entity, not an object. A behavior is a component on
            // an entity and has no identity apart from it, so there is nothing
            // else `this` could usefully be — and typing it as the entity is
            // what makes `Despawn(this)` and `SetParent(this, …)` read the way
            // the API is written, without a conversion nobody would expect to
            // need.
            Expr::This(span) => {
                if context.owner.is_none() {
                    // A free function has no subject. Refused here rather than
                    // faulting at run time, which is the difference between a
                    // message naming the line and a behavior that stops.
                    self.error_note(
                        "`this` names the entity a behavior is attached to",
                        *span,
                        "a free function has no entity — take an `Entity` parameter instead",
                    );
                    return Ty::Error;
                }
                Ty::Entity
            }

            Expr::Ident { name, span } => match self.scopes.type_of(name) {
                Some(ty) => ty,
                None => {
                    // A known function used without calling it is a common
                    // slip, and worth naming as such.
                    if self.functions.contains_key(name) {
                        self.error_note(
                            format!("`{name}` is a function"),
                            *span,
                            "call it: `{name}(…)`",
                        );
                    } else {
                        self.error(format!("`{name}` is not declared"), *span);
                    }
                    Ty::Error
                }
            },

            Expr::ArrayLit { elements, span } => self.check_array(elements, *span, context),
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(*op, lhs, rhs, *span, context),
            Expr::Unary { op, operand, span } => self.check_unary(*op, operand, *span, context),
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => self.check_assign(target, *op, value, *span, context),
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span, context),
            Expr::Field { object, name, span } => {
                let receiver = self.check_expr(object, context);
                self.check_field(&receiver, name, *span, false)
            }
            Expr::OptionalField { object, name, span } => {
                let receiver = self.check_expr(object, context);
                if !receiver.is_optional() && !matches!(receiver, Ty::Error) {
                    self.error_note(
                        format!("`{}` is never null", receiver.name()),
                        *span,
                        "use `.` — `?.` is for optionals, and using it here suggests a doubt the type says is unfounded",
                    );
                }
                // The result is optional whatever the field's own type: the
                // receiver may be absent.
                let inner = receiver.unwrapped();
                let field = self.check_field(&inner, name, *span, true);
                match field {
                    Ty::Error => Ty::Error,
                    other => Ty::Optional(Box::new(other)),
                }
            }
            Expr::Index {
                object,
                index,
                span,
            } => self.check_index(object, index, *span, context),
            Expr::Ternary {
                condition,
                then_value,
                else_value,
                span,
            } => {
                let condition_ty = self.check_expr(condition, context);
                self.expect_bool(&condition_ty, condition.span(), "a conditional");
                let then_ty = self.check_expr(then_value, context);
                let else_ty = self.check_expr(else_value, context);
                self.unify(
                    &then_ty,
                    &else_ty,
                    *span,
                    "the two branches of a conditional",
                )
            }
            Expr::New { ty, args, span } => {
                let resolved = self.resolve(ty);
                for arg in args {
                    self.check_expr(arg, context);
                }
                let _ = span;
                resolved
            }
            // Reached only when a binding appears outside a condition, where
            // `check_if` would have intercepted it. Report rather than infer a
            // type for something that has no meaning here.
            Expr::Binding { value, span, .. } => {
                self.check_expr(value, context);
                self.error_note(
                    "`var` here is not a condition",
                    *span,
                    "the binding form belongs in an `if` or `while` condition, where it tests an optional and narrows it for the branch",
                );
                Ty::Error
            }
            Expr::Await { operand, span } => self.check_await(operand, *span, context),
            Expr::Cast { ty, operand, span } => {
                let from = self.check_expr(operand, context);
                let to = self.resolve(ty);
                self.check_cast(&from, &to, *span);
                to
            }
        }
    }

    fn check_array(&mut self, elements: &[Expr], span: Span, context: &Context) -> Ty {
        let Some((first, rest)) = elements.split_first() else {
            // An empty literal has no element type to infer; the annotation on
            // the binding supplies it.
            return Ty::Array(Box::new(Ty::Error));
        };
        let mut element = self.check_expr(first, context);
        for other in rest {
            let other_ty = self.check_expr(other, context);
            element = self.unify(&element, &other_ty, span, "the elements of an array");
        }
        Ty::Array(Box::new(element))
    }

    fn check_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        context: &Context,
    ) -> Ty {
        let left = self.check_expr(lhs, context);
        let right = self.check_expr(rhs, context);

        if matches!(left, Ty::Error) || matches!(right, Ty::Error) {
            return Ty::Error;
        }

        match op {
            BinaryOp::And | BinaryOp::Or => {
                self.expect_bool(&left, lhs.span(), "a logical operator");
                self.expect_bool(&right, rhs.span(), "a logical operator");
                Ty::Bool
            }
            BinaryOp::Coalesce => {
                if !left.is_optional() {
                    self.error_note(
                        format!("`{}` is never null", left.name()),
                        lhs.span(),
                        "`??` supplies a fallback for an optional; this value always has one",
                    );
                    return left;
                }
                let present = left.unwrapped();
                self.expect_assignable(&present, &right, rhs.span());
                present
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if let Some(result) = self.overloaded(op, &left, &right, span) {
                    return result;
                }
                if !left.accepts(&right) && !right.accepts(&left) {
                    self.error_note(
                        format!("cannot compare `{}` with `{}`", left.name(), right.name()),
                        span,
                        "equality needs both sides to be the same type",
                    );
                }
                Ty::Bool
            }
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq => {
                if let Some(result) = self.overloaded(op, &left, &right, span) {
                    return result;
                }
                let comparable = (left.is_numeric() || left.is_unit()) && left == right
                    || left.is_numeric() && right.is_numeric();
                if !comparable {
                    self.error(
                        format!("cannot order `{}` against `{}`", left.name(), right.name()),
                        span,
                    );
                }
                Ty::Bool
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if let Some(result) = self.overloaded(op, &left, &right, span) {
                    return result;
                }
                self.arithmetic(op, &left, &right, span)
            }
        }
    }

    /// The result of arithmetic on two built-in types.
    ///
    /// The unit rules live here, and they are deliberately narrow: a unit may
    /// be added to its own kind, scaled by a number, or divided by its own kind
    /// to yield a ratio. Everything else has no meaning worth guessing at.
    fn arithmetic(&mut self, op: BinaryOp, left: &Ty, right: &Ty, span: Span) -> Ty {
        // Plain numbers.
        if left.is_numeric() && right.is_numeric() {
            return if *left == Ty::Float || *right == Ty::Float {
                Ty::Float
            } else {
                Ty::Int
            };
        }

        if left.is_unit() || right.is_unit() {
            return self.unit_arithmetic(op, left, right, span);
        }

        // Engine vector types scale by a number and combine with themselves.
        if let Ty::Engine(name) = left {
            if right.is_numeric() || left == right {
                return Ty::Engine(name);
            }
        }
        if let (true, Ty::Engine(name)) = (left.is_numeric(), right) {
            return Ty::Engine(name);
        }

        if *left == Ty::Str && op == BinaryOp::Add {
            self.expect_assignable(&Ty::Str, right, span);
            return Ty::Str;
        }

        self.error(
            format!(
                "`{}` cannot be applied to `{}` and `{}`",
                operator_text(op),
                left.name(),
                right.name()
            ),
            span,
        );
        Ty::Error
    }

    /// Arithmetic where at least one side carries a unit.
    fn unit_arithmetic(&mut self, op: BinaryOp, left: &Ty, right: &Ty, span: Span) -> Ty {
        match op {
            // Same unit in, same unit out.
            BinaryOp::Add | BinaryOp::Sub if left == right => left.clone(),

            BinaryOp::Add | BinaryOp::Sub => {
                self.error_note(
                    format!("cannot add `{}` to `{}`", right.name(), left.name()),
                    span,
                    "Duration and Angle measure different things; there is no meaningful sum",
                );
                Ty::Error
            }

            // Scaling keeps the unit.
            BinaryOp::Mul if left.is_unit() && right.is_numeric() => left.clone(),
            BinaryOp::Mul if left.is_numeric() && right.is_unit() => right.clone(),
            BinaryOp::Mul => {
                self.error_note(
                    format!("cannot multiply `{}` by `{}`", left.name(), right.name()),
                    span,
                    "a unit may be scaled by a number; multiplying two units would give an area, which the language has no type for",
                );
                Ty::Error
            }

            // Dividing a unit by its own kind is a ratio — a plain number.
            BinaryOp::Div if left == right => Ty::Float,
            BinaryOp::Div if left.is_unit() && right.is_numeric() => left.clone(),
            BinaryOp::Div => {
                self.error_note(
                    format!("cannot divide `{}` by `{}`", left.name(), right.name()),
                    span,
                    "divide a unit by a number to scale it, or by its own kind to get a ratio",
                );
                Ty::Error
            }

            BinaryOp::Rem => {
                self.error(format!("`%` cannot be applied to `{}`", left.name()), span);
                Ty::Error
            }

            _ => Ty::Error,
        }
    }

    /// Looks for an operator overload matching these operand types.
    ///
    /// Resolution happens here, at check time, so the emitted call is direct.
    fn overloaded(&mut self, op: BinaryOp, left: &Ty, right: &Ty, span: Span) -> Option<Ty> {
        let overloadable = op.overloadable()?;
        let Ty::Struct(name) = left else {
            return None;
        };
        let info = self.structs.get(name)?;

        let found = info
            .operators
            .iter()
            .find(|candidate| {
                candidate.op == overloadable
                    && candidate.params.len() == 2
                    && candidate.params[0].accepts(left)
                    && candidate.params[1].accepts(right)
            })
            .map(|candidate| candidate.result.clone());

        if found.is_none() {
            // The struct exists but has no such overload: say so precisely,
            // rather than falling through to "cannot be applied".
            self.error_note(
                format!(
                    "`{}` does not define `{}` for `{}`",
                    name,
                    operator_text(op),
                    right.name()
                ),
                span,
                format!(
                    "declare it: `static {} operator {}({} a, {} b)`",
                    name,
                    operator_text(op),
                    name,
                    right.name()
                ),
            );
            return Some(Ty::Error);
        }
        found
    }

    fn check_unary(&mut self, op: UnaryOp, operand: &Expr, span: Span, context: &Context) -> Ty {
        let ty = self.check_expr(operand, context);
        if matches!(ty, Ty::Error) {
            return Ty::Error;
        }
        match op {
            UnaryOp::Not => {
                self.expect_bool(&ty, span, "`!`");
                Ty::Bool
            }
            UnaryOp::Neg => {
                if ty.is_numeric() || ty.is_unit() || matches!(ty, Ty::Engine(_)) {
                    return ty;
                }
                self.error(format!("cannot negate `{}`", ty.name()), span);
                Ty::Error
            }
        }
    }

    fn check_assign(
        &mut self,
        target: &Expr,
        op: Option<BinaryOp>,
        value: &Expr,
        span: Span,
        context: &Context,
    ) -> Ty {
        if !is_assignable_target(target) {
            self.error_note(
                "this cannot be assigned to",
                target.span(),
                "only a variable, a field or an element may appear on the left of `=`",
            );
            self.check_expr(value, context);
            return Ty::Error;
        }

        let target_ty = self.check_expr(target, context);
        let value_ty = self.check_expr(value, context);

        match op {
            // `a += b` must mean what `a = a + b` means, overloads included.
            Some(op) => {
                let result =
                    if let Some(overloaded) = self.overloaded(op, &target_ty, &value_ty, span) {
                        overloaded
                    } else {
                        self.arithmetic(op, &target_ty, &value_ty, span)
                    };
                self.expect_assignable(&target_ty, &result, span);
            }
            None => self.expect_assignable(&target_ty, &value_ty, value.span()),
        }

        target_ty
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], span: Span, context: &Context) -> Ty {
        // Free functions and engine functions are callable by name; methods on
        // engine objects arrive with the script lane.
        if let Expr::Ident { name, .. } = callee {
            // The script's own first, so a host that later exposes a name a
            // script already uses cannot change what that script means.
            let known = self
                .functions
                .get(name)
                .or_else(|| self.natives.get(name))
                .cloned();
            if let Some(info) = known {
                let wrong_arity = if info.variadic {
                    args.len() < info.params.len()
                } else {
                    args.len() != info.params.len()
                };
                if wrong_arity {
                    self.error(
                        format!(
                            "`{name}` takes {}{} argument{}, found {}",
                            if info.variadic { "at least " } else { "" },
                            info.params.len(),
                            if info.params.len() == 1 { "" } else { "s" },
                            args.len()
                        ),
                        span,
                    );
                }
                for (argument, expected) in args.iter().zip(info.params.iter()) {
                    let actual = self.check_expr(argument, context);
                    self.expect_assignable(expected, &actual, argument.span());
                }
                // Arguments beyond the declared ones still get checked, so
                // their own mistakes are reported too.
                for extra in args.iter().skip(info.params.len()) {
                    self.check_expr(extra, context);
                }
                return info.result;
            }
        }

        // Unknown callee: check the arguments so their errors surface, and
        // yield `Error` without a second complaint about the callee itself —
        // `check_expr` on the callee has already reported it.
        self.check_expr(callee, context);
        for argument in args {
            self.check_expr(argument, context);
        }
        Ty::Error
    }

    fn check_field(&mut self, receiver: &Ty, name: &str, span: Span, optional: bool) -> Ty {
        match receiver {
            Ty::Error => Ty::Error,
            Ty::Struct(struct_name) => {
                match self
                    .structs
                    .get(struct_name)
                    .and_then(|info| info.fields.get(name))
                {
                    Some(ty) => ty.clone(),
                    None => {
                        self.error(format!("`{struct_name}` has no field `{name}`"), span);
                        Ty::Error
                    }
                }
            }
            Ty::Array(_) if name == "Length" => Ty::Int,
            Ty::Str if name == "Length" => Ty::Int,
            Ty::Duration if name == "Seconds" => Ty::Float,
            Ty::Angle if name == "Radians" => Ty::Float,
            Ty::Angle if name == "Degrees" => Ty::Float,
            // Engine types and `this` are opened up in phase 2, when the API
            // surface is declared. Until then anything on them is accepted
            // rather than wrongly rejected.
            Ty::Engine(_) | Ty::Behavior(_) | Ty::Entity => Ty::Error,
            other if optional => {
                self.error(format!("`{}` has no field `{name}`", other.name()), span);
                Ty::Error
            }
            other => {
                self.error(format!("`{}` has no field `{name}`", other.name()), span);
                Ty::Error
            }
        }
    }

    fn check_index(&mut self, object: &Expr, index: &Expr, span: Span, context: &Context) -> Ty {
        let collection = self.check_expr(object, context);
        let index_ty = self.check_expr(index, context);

        match &collection {
            Ty::Array(element) => {
                self.expect_assignable(&Ty::Int, &index_ty, index.span());
                (**element).clone()
            }
            Ty::Map(key, value) => {
                self.expect_assignable(key, &index_ty, index.span());
                (**value).clone()
            }
            Ty::Error => Ty::Error,
            other => {
                self.error(format!("`{}` cannot be indexed", other.name()), span);
                Ty::Error
            }
        }
    }

    /// `await` — legal only where the member can actually suspend.
    fn check_await(&mut self, operand: &Expr, span: Span, context: &Context) -> Ty {
        let ty = self.check_expr(operand, context);

        if !context.allows_await {
            self.error_note(
                format!("`await` is not allowed in `{}`", context.member),
                span,
                "mark the member `async`. `Update` and `every` run every frame, so a suspension there would pile up one continuation per frame and per entity",
            );
            return Ty::Error;
        }

        match ty {
            // Awaiting a duration yields nothing; it is the waiting that
            // matters.
            Ty::Duration | Ty::Error => Ty::Void,
            // Awaiting anything else is a handle whose value is the result.
            // Until the engine API lands there is nothing more to say.
            other => other,
        }
    }

    fn check_cast(&mut self, from: &Ty, to: &Ty, span: Span) {
        let allowed = matches!(
            (from, to),
            (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) | (Ty::Error, _) | (_, Ty::Error)
        );
        if !allowed {
            self.error_note(
                format!("cannot cast `{}` to `{}`", from.name(), to.name()),
                span,
                "casts convert between int and float; other conversions go through a named accessor",
            );
        }
    }

    /// The type both sides must share, reporting when they cannot.
    pub fn unify(&mut self, left: &Ty, right: &Ty, span: Span, what: &str) -> Ty {
        if left.accepts(right) {
            return left.clone();
        }
        if right.accepts(left) {
            return right.clone();
        }
        self.error(
            format!("{what} disagree: `{}` and `{}`", left.name(), right.name()),
            span,
        );
        Ty::Error
    }

    /// Reports when a value is used as a condition without being a `bool`.
    pub fn expect_bool(&mut self, ty: &Ty, span: Span, what: &str) {
        if matches!(ty, Ty::Bool | Ty::Error) {
            return;
        }
        if ty.is_optional() {
            // Testing an optional for presence is idiomatic, so this is a
            // shape question rather than a type error.
            self.error_note(
                format!("{what} needs a bool, found `{}`", ty.name()),
                span,
                "bind it to test it: `if (var x = value)` gives a narrowed `x` inside the branch",
            );
            return;
        }
        self.error(format!("{what} needs a bool, found `{}`", ty.name()), span);
    }
}

/// Whether an expression denotes a place that can be written to.
fn is_assignable_target(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident { .. } | Expr::Field { .. } | Expr::Index { .. }
    )
}

/// How an operator is spelled, for messages.
fn operator_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Eq => "==",
        BinaryOp::NotEq => "!=",
        BinaryOp::Less => "<",
        BinaryOp::LessEq => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEq => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        BinaryOp::Coalesce => "??",
    }
}
