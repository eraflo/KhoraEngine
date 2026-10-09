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

//! `receiver.Method(…)` and `receiver.name` where the receiver has no fields.
//!
//! The only methods today are an array's `Push` and `RemoveAt`, which change
//! the array in place — so the receiver must be a *place*: a variable, a field,
//! or an element or struct field of one. An array nothing keeps (`Make()`,
//! `[1]`) would grow and be thrown away. An entity has neither fields nor
//! methods: a behavior's own are named directly.

use super::{Checker, Context, Ty};
use crate::ast::Expr;
use crate::diagnostics::Span;

/// What an array can be asked to do.
const ARRAY_METHODS: &[&str] = &["Push", "RemoveAt"];

impl Checker {
    /// `object.name(args)` and `object?.name(args)`, or `None` when the callee
    /// is not a method call.
    pub(super) fn check_method_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        context: &Context,
    ) -> Option<Ty> {
        let (object, name, at, optional) = match callee {
            Expr::Field { object, name, span } => (object, name, *span, false),
            Expr::OptionalField { object, name, span } => (object, name, *span, true),
            _ => return None,
        };
        let checked = self.check_expr(object, context);
        if optional && !checked.is_optional() && !matches!(checked, Ty::Error) {
            self.error_note(
                format!("`{}` is never null", checked.name()),
                at,
                "use `.` — `?.` is for optionals, and using it here suggests a doubt the type says is unfounded",
            );
        }
        let receiver = if optional {
            checked.unwrapped()
        } else {
            checked
        };
        if let Ty::Array(element) = &receiver {
            return Some(self.check_array_method(object, name, at, element, args, context));
        }
        for argument in args {
            self.check_expr(argument, context);
        }
        if receiver == Ty::Entity {
            let own =
                matches!(**object, Expr::This(_)) && self.method_names.contains(name.as_str());
            match (&context.owner, own) {
                (Some(owner), true) => self.error(
                    format!("`{name}` is a method of `{owner}`: call it by name — `{name}()`"),
                    at,
                ),
                _ => self.error_note(
                    format!("an entity has no method `{name}`"),
                    at,
                    "calling a method on an entity is not available yet — a behavior's own methods are called by name",
                ),
            }
        } else if !matches!(receiver, Ty::Error) {
            self.error_note(
                format!("`{}` has no method `{name}`", receiver.name()),
                at,
                "only an array has methods today: `Push` and `RemoveAt`",
            );
        }
        Some(Ty::Error)
    }

    /// `xs.Push(v)` and `xs.RemoveAt(i)`.
    fn check_array_method(
        &mut self,
        object: &Expr,
        name: &str,
        at: Span,
        element: &Ty,
        args: &[Expr],
        context: &Context,
    ) -> Ty {
        let wanted = match name {
            "Push" => element.clone(),
            "RemoveAt" => Ty::Int,
            _ => {
                for argument in args {
                    self.check_expr(argument, context);
                }
                self.error_note(
                    format!("an array has no method `{name}`"),
                    at,
                    format!("its methods: {}", ARRAY_METHODS.join(", ")),
                );
                return Ty::Error;
            }
        };
        if args.len() != 1 {
            self.error(
                format!("`{name}` takes 1 argument, found {}", args.len()),
                at,
            );
        }
        for (position, argument) in args.iter().enumerate() {
            let actual = self.check_expr(argument, context);
            if position == 0 {
                self.expect_assignable(&wanted, &actual, argument.span());
            }
        }
        if !is_place(object) {
            self.error_note(
                format!("`{name}` changes an array in place, and this one is kept nowhere"),
                at,
                "call it on a variable, a field, or an element or field of one",
            );
        }
        Ty::Void
    }

    /// `entity.name`: an entity has no fields — reported at the line, about
    /// what was written.
    pub(super) fn entity_field(
        &mut self,
        object: &Expr,
        name: &str,
        span: Span,
        context: &Context,
    ) -> Ty {
        let own =
            matches!(object, Expr::This(_)) && self.field_names.iter().any(|field| field == name);
        match (&context.owner, own) {
            (Some(owner), true) => self.error(
                format!("`{name}` is a field of `{owner}`: name it directly"),
                span,
            ),
            _ => self.error_note(
                format!("an entity has no field `{name}`"),
                span,
                "a component is read with `Get` on the entity — reading components from a script is not available yet",
            ),
        }
        Ty::Error
    }
}

/// Whether `expr` names somewhere a value is kept: a variable or a field,
/// or an element or struct field of one.
fn is_place(expr: &Expr) -> bool {
    match expr {
        Expr::Ident { .. } => true,
        Expr::Index { object, .. } | Expr::Field { object, .. } => is_place(object),
        _ => false,
    }
}
