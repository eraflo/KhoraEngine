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

//! Arithmetic and operators: numbers, units, engine types, overloads.
//!
//! An engine type's operation is typed by the engine function that performs
//! it — `Vec3 + Vec3` is the native `"Vec3 + Vec3"` — and refused, by name,
//! when no such function exists (see
//! [`native::operators`](crate::native::operators)).

use super::{Checker, Ty};
use crate::ast::BinaryOp;
use crate::diagnostics::Span;

impl Checker {
    /// The result of arithmetic on two built-in types.
    ///
    /// The unit rules live here, and they are deliberately narrow: a unit may
    /// be added to its own kind, scaled by a number, or divided by its own kind
    /// to yield a ratio. Everything else has no meaning worth guessing at.
    pub(super) fn arithmetic(&mut self, op: BinaryOp, left: &Ty, right: &Ty, span: Span) -> Ty {
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

        // An engine type's operations are engine functions: the operation is
        // defined exactly when its native exists.
        let engine = match (left, right) {
            (Ty::Engine(name), _) | (_, Ty::Engine(name)) => Some(*name),
            _ => None,
        };
        if let Some(engine) = engine {
            let name = crate::native::operators::binary_operator(
                &operand_name(left),
                operator_text(op),
                &operand_name(right),
            );
            return self.engine_operator(name, engine, span);
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

    /// The type of the engine-type operation the native `name` performs, or
    /// an error naming the operation and listing what `engine` does define.
    pub(super) fn engine_operator(&mut self, name: String, engine: &str, span: Span) -> Ty {
        if let Some(native) = self.natives.get(&name) {
            return native.result.clone();
        }
        let mut defined: Vec<&str> = self
            .natives
            .keys()
            .filter(|known| {
                known.contains(' ') && known.split(' ').any(|operand| operand == engine)
            })
            .map(String::as_str)
            .collect();
        defined.sort_unstable();
        let note = if defined.is_empty() {
            format!("`{engine}` defines no arithmetic")
        } else {
            format!("`{engine}` defines: {}", defined.join(", "))
        };
        self.error_note(format!("`{name}` is not defined"), span, note);
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
    pub(super) fn overloaded(
        &mut self,
        op: BinaryOp,
        left: &Ty,
        right: &Ty,
        span: Span,
    ) -> Option<Ty> {
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
}

/// How an operand is named in an operator native: an engine type by its name,
/// a number as `float` — an `int` widens where a number is expected.
fn operand_name(ty: &Ty) -> String {
    match ty {
        Ty::Engine(name) => (*name).to_owned(),
        Ty::Int | Ty::Float => "float".to_owned(),
        other => other.name(),
    }
}

/// How an operator is spelled, for messages.
pub(super) fn operator_text(op: BinaryOp) -> &'static str {
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
