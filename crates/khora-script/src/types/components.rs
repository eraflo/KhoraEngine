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

//! Operations on an entity: `e.SetPosition(v)`, `e.Set(C { … })`, `Spawn(…)`.
//!
//! An engine operation whose first parameter is an `Entity` is called *on*
//! that entity — one spelling, so typing `this.` lists everything that can be
//! done to it. The free form of such an operation is refused with its rewrite.
//!
//! `Set`, `Add` and `Remove` write components; they are intrinsics rather than
//! natives because they take what a native cannot — a component literal, a
//! component name. A component literal is a *patch*: the fields it names are
//! written, the others are left as they are.

use std::collections::HashMap;

use super::{Checker, Context, Ty};
use crate::ast::{BinaryOp, Expr, StructDecl, UnaryOp};
use crate::diagnostics::Span;

/// The names the language keeps for its operations on an entity: no function
/// or method may take one.
pub(crate) const RESERVED: &[&str] = &["Set", "Add", "Remove", "Get", "Has", "Spawn"];

/// A component a script may write, by field.
#[derive(Debug, Clone, Default)]
pub struct ComponentInfo {
    /// Each field's type.
    pub fields: HashMap<String, Ty>,
}

impl Checker {
    /// Records a `component` declaration.
    pub(super) fn collect_component(&mut self, decl: &StructDecl) {
        let fields = decl
            .fields
            .iter()
            .map(|field| (field.name.clone(), self.resolve(&field.ty)))
            .collect();
        self.components
            .insert(decl.name.clone(), ComponentInfo { fields });
    }

    /// Refuses a reserved name as a function's or a method's.
    pub(super) fn refuse_reserved(&mut self, name: &str, span: Span) -> bool {
        if !RESERVED.contains(&name) {
            return false;
        }
        self.error_note(
            format!("`{name}` is reserved"),
            span,
            "the language names its operations on entities with it — `e.Set(…)`, `e.Get(…)`, `Spawn(…)`",
        );
        true
    }

    /// `object.name(args)` where `object` is an entity.
    pub(super) fn check_entity_method(
        &mut self,
        object: &Expr,
        name: &str,
        at: Span,
        args: &[Expr],
        context: &Context,
    ) -> Option<Ty> {
        match name {
            "Set" | "Add" => {
                self.check_patch_arguments(name, args, at, context);
                return Some(Ty::Void);
            }
            "Remove" => {
                self.check_removed(args, at, context);
                return Some(Ty::Void);
            }
            "Get" | "Has" => {
                for argument in args {
                    self.check_expr(argument, context);
                }
                self.error_note(
                    format!("`{name}` on an entity is not available yet"),
                    at,
                    "reading a component from a script comes with the next step of the language",
                );
                return Some(Ty::Error);
            }
            "Spawn" => {
                for argument in args {
                    self.check_expr(argument, context);
                }
                self.error_note(
                    "`Spawn` creates an entity: it is not called on one",
                    at,
                    "write `Spawn(position, …)`",
                );
                return Some(Ty::Error);
            }
            _ => {}
        }
        let info = self.natives.get(name).cloned()?;
        if info.params.first() != Some(&Ty::Entity) {
            return None;
        }
        let rest = &info.params[1..];
        let wrong_arity = if info.variadic {
            args.len() < rest.len()
        } else {
            args.len() != rest.len()
        };
        if wrong_arity {
            self.error(
                format!(
                    "`{name}` takes {}{} argument{}, found {}",
                    if info.variadic { "at least " } else { "" },
                    rest.len(),
                    if rest.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                at,
            );
        }
        // The receiver is the first argument; it was checked as the callee's
        // object.
        let _ = object;
        for (argument, expected) in args.iter().zip(rest) {
            let actual = self.check_expr(argument, context);
            self.expect_assignable(expected, &actual, argument.span());
        }
        for extra in args.iter().skip(rest.len()) {
            self.check_expr(extra, context);
        }
        Some(info.result)
    }

    /// The free form of an entity operation, refused with its rewrite —
    /// `true` when `name` is one.
    pub(super) fn refuse_free_form(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        context: &Context,
    ) -> bool {
        let Some(info) = self.natives.get(name) else {
            return false;
        };
        if info.params.first() != Some(&Ty::Entity) {
            return false;
        }
        let rewrite = match args.split_first() {
            Some((receiver, rest)) => format!(
                "{}.{name}({})",
                sketch_receiver(receiver),
                rest.iter().map(sketch).collect::<Vec<_>>().join(", ")
            ),
            None => format!("this.{name}()"),
        };
        for argument in args {
            self.check_expr(argument, context);
        }
        self.error_note(
            format!("Write `{rewrite}`"),
            span,
            format!("`{name}` is called on the entity it acts on"),
        );
        true
    }

    /// `Spawn(Vec3 position, C { … }, …)`.
    pub(super) fn check_spawn(&mut self, args: &[Expr], span: Span, context: &Context) -> Ty {
        let Some((position, components)) = args.split_first() else {
            self.error(
                "`Spawn` takes a position, then the components to attach",
                span,
            );
            return Ty::Void;
        };
        let at = self.check_expr(position, context);
        self.expect_assignable(&Ty::Engine("Vec3"), &at, position.span());
        let mut seen: Vec<&str> = Vec::new();
        for component in components {
            if let Some(name) = self.check_patch(component, "Spawn", context) {
                if seen.contains(&name) {
                    self.error(
                        format!("`{name}` is attached twice: an entity holds one of each"),
                        component.span(),
                    );
                }
                seen.push(name);
            }
        }
        Ty::Void
    }

    /// The one component literal of `Set` or `Add`.
    fn check_patch_arguments(&mut self, name: &str, args: &[Expr], at: Span, context: &Context) {
        if args.len() != 1 {
            self.error(
                format!("`{name}` takes 1 argument, found {}", args.len()),
                at,
            );
        }
        if let Some(first) = args.first() {
            self.check_patch(first, name, context);
        }
        for extra in args.iter().skip(1) {
            self.check_expr(extra, context);
        }
    }

    /// A component literal given to `operation`: every field named exists and
    /// takes its value. Returns the component's name when it is one.
    fn check_patch<'e>(
        &mut self,
        expr: &'e Expr,
        operation: &str,
        context: &Context,
    ) -> Option<&'e str> {
        let Expr::StructLit {
            name,
            name_span,
            fields,
            ..
        } = expr
        else {
            self.check_expr(expr, context);
            self.error_note(
                format!("`{operation}` takes a component literal"),
                expr.span(),
                "write the component and the fields to change — `RigidBody { mass: 12.0 }`",
            );
            return None;
        };
        self.types
            .insert(std::ptr::from_ref(expr) as usize, Ty::Void);
        let Some(info) = self.components.get(name).cloned() else {
            for (_, _, value) in fields {
                self.check_expr(value, context);
            }
            self.not_a_component(name, *name_span);
            return None;
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
                None => self.error(format!("`{name}` has no field `{field}`"), *field_span),
            }
        }
        Some(name)
    }

    /// `e.Remove(C)`: a bare component name.
    fn check_removed(&mut self, args: &[Expr], at: Span, context: &Context) {
        if args.len() != 1 {
            self.error(
                format!("`Remove` takes 1 argument, found {}", args.len()),
                at,
            );
        }
        for (position, argument) in args.iter().enumerate() {
            match argument {
                Expr::Ident { name, span } if position == 0 => {
                    if !self.components.contains_key(name) {
                        self.not_a_component(name, *span);
                    }
                }
                _ => {
                    self.check_expr(argument, context);
                    if position == 0 {
                        self.error_note(
                            "`Remove` takes a component's name",
                            argument.span(),
                            "write `e.Remove(Collider)` — the whole component goes",
                        );
                    }
                }
            }
        }
    }

    /// `name` is not a component a script writes — with what to do instead.
    fn not_a_component(&mut self, name: &str, span: Span) {
        let (placed, natives) = crate::native::world::PLACEMENT;
        let note = if name == placed {
            format!(
                "`{placed}` is placed with its own operations: {}",
                natives
                    .iter()
                    .map(|native| format!("`{native}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if self.structs.contains_key(name) {
            "the engine writes it, or it has its own operations; only a `component` is written by a script".to_owned()
        } else {
            "the components a script writes come from `engine/components.erg`".to_owned()
        };
        self.error_note(
            format!("`{name}` is not a component a script writes"),
            span,
            note,
        );
    }

    /// A component used where a value goes.
    pub(super) fn component_as_value(&mut self, name: &str, span: Span) {
        self.error_note(
            format!("`{name}` is a component"),
            span,
            format!(
                "write it with `target.Set({name} {{ … }})`, add it with `target.Add({name} {{ … }})`; reading one comes with `target.Get({name})`"
            ),
        );
    }
}

/// `expr` as an author would have written it, for a rewrite in a message.
fn sketch(expr: &Expr) -> String {
    match expr {
        Expr::Int { value, .. } => value.to_string(),
        Expr::Float { value, .. } => format!("{value:?}"),
        Expr::Str { value, .. } => format!("{value:?}"),
        Expr::Bool { value, .. } => value.to_string(),
        Expr::Null(_) => "null".to_owned(),
        Expr::This(_) => "this".to_owned(),
        Expr::Ident { name, .. } => name.clone(),
        Expr::Field { object, name, .. } => format!("{}.{name}", sketch_receiver(object)),
        Expr::OptionalField { object, name, .. } => {
            format!("{}?.{name}", sketch_receiver(object))
        }
        Expr::Index { object, index, .. } => {
            format!("{}[{}]", sketch_receiver(object), sketch(index))
        }
        Expr::Call { callee, args, .. } => format!(
            "{}({})",
            sketch(callee),
            args.iter().map(sketch).collect::<Vec<_>>().join(", ")
        ),
        Expr::Unary { op, operand, .. } => {
            let symbol = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{symbol}{}", sketch(operand))
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            format!("{} {} {}", sketch(lhs), symbol(*op), sketch(rhs))
        }
        _ => "…".to_owned(),
    }
}

/// `expr` as the left of a `.`: an operation is parenthesised, so the rewrite
/// calls the method on what the author wrote, not on its last operand.
fn sketch_receiver(expr: &Expr) -> String {
    match expr {
        Expr::Binary { .. }
        | Expr::Unary { .. }
        | Expr::Ternary { .. }
        | Expr::Assign { .. }
        | Expr::Cast { .. } => format!("({})", sketch(expr)),
        _ => sketch(expr),
    }
}

fn symbol(op: BinaryOp) -> &'static str {
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
