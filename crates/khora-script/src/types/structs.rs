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

//! Struct values: the literal, and what a struct's defaults may be.
//!
//! `Loot { value: 1 }` names each field it writes, in any order; a field it
//! leaves out takes its declared default, or its type's zero — and a field
//! with neither must be written. A struct's defaults are **constants**: they
//! are data the engine reads wherever a value of the struct is built or
//! loaded, not code that runs, so a saved value missing a field can be filled
//! from them without running anything.

use super::{Checker, Context, Ty};
use crate::ast::{Expr, UnaryOp};
use crate::diagnostics::Span;

impl Checker {
    /// `Name { field: value, … }`.
    pub(super) fn check_struct_lit(
        &mut self,
        name: &str,
        name_span: Span,
        fields: &[(String, Span, Expr)],
        span: Span,
        context: &Context,
    ) -> Ty {
        let Some(info) = self.structs.get(name).cloned() else {
            let note = if crate::types::ty::ENGINE_TYPES.contains(&name) {
                format!("an engine type is built with its function — `{name}(…)`")
            } else {
                "declare it with `struct`".to_owned()
            };
            self.error_note(format!("no struct named `{name}`"), name_span, note);
            for (_, _, value) in fields {
                self.check_expr(value, context);
            }
            return Ty::Error;
        };

        let mut written: Vec<&str> = Vec::new();
        for (field, field_span, value) in fields {
            let actual = self.check_expr(value, context);
            if written.contains(&field.as_str()) {
                self.error(format!("field `{field}` is written twice"), *field_span);
                continue;
            }
            written.push(field);
            match info.fields.get(field) {
                Some(declared) => self.expect_assignable(declared, &actual, value.span()),
                None => self.error_note(
                    format!("`{name}` has no field `{field}`"),
                    *field_span,
                    format!("its fields: {}", info.order.join(", ")),
                ),
            }
        }

        for field in &info.order {
            if written.contains(&field.as_str()) || info.defaulted.contains(field) {
                continue;
            }
            let ty = info.fields.get(field).cloned().unwrap_or(Ty::Error);
            if !super::stmt::has_zero(&ty) {
                self.error_note(
                    format!("field `{field}` needs a value"),
                    span,
                    format!(
                        "`{}` has no default and no zero — write it, or give the field a default in `{name}`",
                        ty.name()
                    ),
                );
            }
        }
        Ty::Struct(name.to_owned())
    }

    /// Refuses a struct field default that is not a constant.
    pub(super) fn expect_constant_default(&mut self, default: &Expr) {
        if is_constant(default) {
            return;
        }
        self.error_note(
            "a struct field's default must be a constant",
            default.span(),
            "a struct's defaults are data, read wherever one is built or loaded — write a number, text, `null`, a duration, an angle, or `[…]` / `Name { … }` of those",
        );
    }
}

impl Checker {
    /// Refuses a default that, through the defaults of the literals it
    /// builds, builds itself again — `struct A { A? next = A { v: 1 }; }`
    /// leaves `next` out of its own literal, whose `next` is the same default.
    pub(super) fn expect_finite_default(&mut self, owner: &str, field: &str, default: &Expr) {
        let start = (owner.to_owned(), field.to_owned());
        let mut pending = Vec::new();
        self.omitted_defaults(default, &mut pending);
        let mut seen: Vec<(String, String)> = Vec::new();
        while let Some(next) = pending.pop() {
            if next == start {
                self.error_note(
                    format!("the default of `{owner}.{field}` builds itself without end"),
                    default.span(),
                    "a literal in it leaves out a field whose default needs this one again — write that field, or leave it `null`",
                );
                return;
            }
            if seen.contains(&next) {
                continue;
            }
            let expr = self
                .structs
                .get(&next.0)
                .and_then(|info| info.default_exprs.get(&next.1))
                .cloned();
            seen.push(next);
            if let Some(expr) = expr {
                self.omitted_defaults(&expr, &mut pending);
            }
        }
    }

    /// The `(struct, field)` defaults building `expr` needs: the fields its
    /// struct literals leave out that declare a default.
    fn omitted_defaults(&self, expr: &Expr, into: &mut Vec<(String, String)>) {
        match expr {
            Expr::StructLit { name, fields, .. } => {
                if let Some(info) = self.structs.get(name) {
                    for defaulted in &info.defaulted {
                        if !fields.iter().any(|(written, _, _)| written == defaulted) {
                            into.push((name.clone(), defaulted.clone()));
                        }
                    }
                }
                for (_, _, value) in fields {
                    self.omitted_defaults(value, into);
                }
            }
            Expr::ArrayLit { elements, .. } => {
                for element in elements {
                    self.omitted_defaults(element, into);
                }
            }
            _ => {}
        }
    }
}

/// Whether `expr` is a constant: a literal, a negated number, or an array or
/// struct literal of constants.
pub(super) fn is_constant(expr: &Expr) -> bool {
    match expr {
        Expr::Int { .. }
        | Expr::Float { .. }
        | Expr::Str { .. }
        | Expr::Bool { .. }
        | Expr::Null(_)
        | Expr::Duration { .. }
        | Expr::Angle { .. } => true,
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
            ..
        } => matches!(
            **operand,
            Expr::Int { .. } | Expr::Float { .. } | Expr::Duration { .. } | Expr::Angle { .. }
        ),
        Expr::ArrayLit { elements, .. } => elements.iter().all(is_constant),
        Expr::StructLit { fields, .. } => fields.iter().all(|(_, _, value)| is_constant(value)),
        _ => false,
    }
}
