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

//! `$"…"` checked: each hole a value the language knows how to show — a
//! number, a duration, an angle, a flag, text, an engine value, an entity, or
//! an optional of one. The string is a `string`.

use super::{Checker, Context, Ty};
use crate::ast::{Expr, InterpPiece};

impl Checker {
    pub(super) fn check_interpolated(&mut self, pieces: &[InterpPiece], context: &Context) -> Ty {
        for piece in pieces {
            let InterpPiece::Hole(hole) = piece else {
                continue;
            };
            let ty = self.check_expr(hole, context);
            if ty == Ty::Void {
                self.error_note(
                    "this hole has nothing to show: it gives no value",
                    hole.span(),
                    "a hole shows a value — call what returns one, or move the call out of the text",
                );
            } else if !shown(&ty) {
                self.error_note(
                    format!(
                        "`$\"…\"` shows numbers, text, flags, engine values and entities — `{}` is a `{}`",
                        what(hole),
                        ty.name()
                    ),
                    hole.span(),
                    "show one of its parts, or a count — `{xs.Length}`, `{loot.value}`",
                );
            }
        }
        Ty::Str
    }
}

/// Whether a value of `ty` can be shown as text.
fn shown(ty: &Ty) -> bool {
    match ty {
        Ty::Int
        | Ty::Float
        | Ty::Duration
        | Ty::Angle
        | Ty::Bool
        | Ty::Str
        | Ty::Entity
        | Ty::Asset(_)
        | Ty::Error => true,
        Ty::Engine(name) => matches!(*name, "Vec2" | "Vec3" | "Vec4" | "Quat" | "Color"),
        Ty::Optional(inner) => shown(inner),
        _ => false,
    }
}

/// How a message names the hole `expr`.
fn what(expr: &Expr) -> String {
    match expr {
        Expr::Ident { name, .. } => name.clone(),
        _ => "this value".to_owned(),
    }
}

/// `$"…"` written for `text + value`: what a refusal of the sum suggests.
pub(super) fn suggested(text: &Expr, value: &Expr) -> String {
    let lead = match text {
        Expr::Str { value, .. } => value.replace('{', "{{").replace('}', "}}"),
        other => format!("{{{}}}", spelled(other)),
    };
    format!("$\"{lead}{{{}}}\"", spelled(value))
}

/// A place written to, as a message repeats it.
pub(super) fn spelled_target(expr: &Expr) -> String {
    spelled(expr)
}

/// `expr` as a hole would hold it, where it is short enough to repeat.
fn spelled(expr: &Expr) -> String {
    match expr {
        Expr::Int { value, .. } => value.to_string(),
        Expr::Float { value, .. } => format!("{value:?}"),
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Ident { name, .. } => name.clone(),
        Expr::Field { object, name, .. } => format!("{}.{name}", spelled(object)),
        Expr::Index { object, index, .. } => format!("{}[{}]", spelled(object), spelled(index)),
        _ => "…".to_owned(),
    }
}
