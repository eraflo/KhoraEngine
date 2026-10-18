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
//! An array's `Push` and `RemoveAt` change the array in place — so the
//! receiver must be a *place*: a variable, a field, or an element or struct
//! field of one. An array nothing keeps (`Make()`, `[1]`) would grow and be
//! thrown away. An entity's methods are the engine's operations on it
//! (`types/components.rs`); it has no fields, and a behavior's own methods are
//! named directly.

use super::{Checker, Context, Ty};
use crate::ast::Expr;
use crate::diagnostics::Span;

/// What an array can be asked to do.
const ARRAY_METHODS: &[&str] = &["Push", "RemoveAt"];

impl Checker {
    pub(super) fn check_call(
        &mut self,
        callee: &Expr,
        call: &super::CallSite<'_>,
        context: &Context,
    ) -> Ty {
        let (args, span) = (call.args, call.span);
        // Free functions and engine functions are callable by name; an engine
        // operation on an entity is called on it, and its free form refused.
        if let Expr::Ident { name, .. } = callee {
            if name == "Spawn" && !self.functions.contains_key(name) {
                return self.check_any_spawn(args, call.labels, span, context);
            }
            if self.events.contains_key(name) {
                for argument in args {
                    self.check_expr(argument, context);
                }
                self.refuse_construction(name, span);
                return Ty::Error;
            }
            if name == "Raise" && !self.functions.contains_key(name) {
                return self.refuse_free_raise(args, span, context);
            }
            if self.system.is_some()
                && crate::native::world::SUBJECT_NATIVES.contains(&name.as_str())
                && !self.functions.contains_key(name)
            {
                self.error_note(
                    format!("`{name}()` reads the entity a behavior is attached to, and a system has none"),
                    span,
                    "a system's entities are the rows of its queries — bind one with `Entity e`",
                );
                for argument in args {
                    self.check_expr(argument, context);
                }
                return Ty::Error;
            }
            self.note_call(name, span);
            if context.owner.is_some() && self.method_names.contains(name) {
                let caller = self.qualified(&context.member);
                let callee = self.qualified(name);
                self.member_calls.entry(caller).or_default().insert(callee);
            }
            if !self.functions.contains_key(name)
                && self.refuse_free_form(name, args, span, context)
            {
                return Ty::Error;
            }
            // The script's own first, so a host that later exposes a name a
            // script already uses cannot change what that script means.
            let known = self
                .functions
                .get(name)
                .or_else(|| self.natives.get(name))
                .cloned();
            if let Some(info) = known {
                let callee = info.callee(name);
                if !info.variadic && !callee.plain(args, call.labels) {
                    if let Some(bound) =
                        self.bind_arguments(&callee, args, call.labels, span, context)
                    {
                        self.bindings.insert(call.at, bound.into());
                    }
                    return info.result;
                }
                if info.variadic {
                    self.refuse_labels(name, call.labels);
                }
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

        // Another state's method: out of scope here, and worth saying where it
        // is rather than that it does not exist.
        if let Expr::Ident { name, .. } = callee {
            if let Some(owners) = self.state_methods.get(name).cloned() {
                let named = owners
                    .iter()
                    .map(|state| format!("`{state}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let states = if owners.len() == 1 { "state" } else { "states" };
                self.error_note(
                    format!("`{name}` belongs to {states} {named}"),
                    span,
                    "it can only be called from inside it",
                );
                for argument in args {
                    self.check_expr(argument, context);
                }
                return Ty::Error;
            }
        }

        // `entity.Method(…)`: an entity has no methods — a behavior's own are
        // called by name.
        if let Some(ty) = self.check_method_call(callee, call, context) {
            return ty;
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

    /// `object.name(args)` and `object?.name(args)`, or `None` when the callee
    /// is not a method call.
    pub(super) fn check_method_call(
        &mut self,
        callee: &Expr,
        call: &super::CallSite<'_>,
        context: &Context,
    ) -> Option<Ty> {
        let args = call.args;
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
            self.refuse_labels(name, call.labels);
            return Some(self.check_array_method(object, name, at, element, args, context));
        }
        if receiver == Ty::Entity && !optional {
            if let Some(ty) = self.check_entity_method(object, name, at, call, context) {
                return Some(ty);
            }
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
                    "an entity's methods are the engine's operations on it — a behavior's own methods are called by name",
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
        // Growing or shrinking an array held by a component's copy changes the
        // copy only — the same trap as assigning to one of its fields.
        if self.refuse_copy_write(object) {
            for argument in args {
                self.check_expr(argument, context);
            }
            return Ty::Error;
        }
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
                format!("a component is read with `Get` on the entity — `e.Get({name})?.field`, or `if (var c = e.Get({name})) {{ … }}`"),
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
