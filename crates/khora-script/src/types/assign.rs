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

//! Assignment: `place = value` and its compound forms — what may be written,
//! and what a write through a copy, a read-only place or a query's binding is
//! told instead.

use super::{Checker, Context, Ty};
use crate::ast::{BinaryOp, Expr};
use crate::diagnostics::Span;

impl Checker {
    pub(super) fn check_assign(
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
        if let Expr::Ident { name, .. } = target {
            if self
                .scopes
                .lookup(name)
                .is_some_and(|binding| binding.query.is_some())
            {
                self.error_note(
                    format!("`{name}` is a query's binding and cannot be reassigned"),
                    span,
                    "it is the row's own — declare a local for another value",
                );
                return Ty::Error;
            }
        }
        if self.refuse_copy_write(target) {
            return Ty::Error;
        }

        // Some places can be read and not written.
        if let Expr::Field { object, name, span } = target {
            let receiver = self
                .types
                .get(&(std::ptr::from_ref(&**object) as usize))
                .cloned()
                .unwrap_or(Ty::Error);
            match receiver {
                Ty::Array(_) | Ty::Str if name == "Length" => {
                    self.error_note(
                        "`Length` is read-only",
                        *span,
                        "it counts what the value holds; change the value to change it",
                    );
                    return Ty::Error;
                }
                Ty::Engine(engine) => {
                    self.error_note(
                        format!("a `{engine}`'s components are read-only"),
                        *span,
                        format!("build a new one — `{engine}(…)` — and assign it"),
                    );
                    return Ty::Error;
                }
                _ => {}
            }
        }

        match op {
            // A target already reported says nothing about the operation.
            Some(_) if matches!(target_ty, Ty::Error) => {}
            // `a += b` must mean what `a = a + b` means, overloads included.
            // Text joins text: the interpolation that shows a number is
            // offered, as for `+`.
            Some(BinaryOp::Add)
                if target_ty == Ty::Str
                    && !matches!(value_ty, Ty::Error)
                    && !Ty::Str.accepts(&value_ty) =>
            {
                self.error_note(
                    format!("expected `string`, found `{}`", value_ty.name()),
                    span,
                    format!(
                        "`+=` joins text to text: write `{} = {}`",
                        super::interpolated::spelled_target(target),
                        super::interpolated::suggested(target, value)
                    ),
                );
            }
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
}

/// Whether an expression denotes a place that can be written to.
fn is_assignable_target(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident { .. } | Expr::Field { .. } | Expr::Index { .. }
    )
}
