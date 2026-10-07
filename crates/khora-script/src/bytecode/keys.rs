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

//! What a statement *says*, as a short stable key.
//!
//! A site is named by the statements around it, and a statement by its own
//! syntax tree — spans left out — rather than by where it sits. Adding a line
//! above it, or editing the body of the loop it belongs to, leaves the key
//! alone; editing the statement itself changes it. A compound statement is
//! keyed by its **header** only (a condition, a `for`'s three clauses), so its
//! body is free to change without renaming every site around it.
//!
//! Every match below is exhaustive on purpose: a node added to the tree must
//! decide what it contributes, or two different statements would share a key.

use crate::ast::{BinaryOp, Expr, Pattern, Stmt, TypeRef, UnaryOp};

/// The kind and key of a statement: `("let", "4b1d0a77")`.
pub fn statement_key(statement: &Stmt) -> (&'static str, String) {
    let mut key = Key::new();
    let kind = key.header(statement);
    (kind, key.hex8())
}

/// The key of a field's default, where an initialiser has no statements.
pub fn field_key(name: &str, default: Option<&Expr>) -> String {
    let mut key = Key::new();
    key.text(name);
    match default {
        Some(expr) => key.expr(expr),
        None => key.tag(0),
    }
    key.hex8()
}

/// The key of a state's entry, inlined where an initialiser enters it.
pub fn state_key(state: &str) -> String {
    let mut key = Key::new();
    key.text(state);
    key.hex8()
}

/// The key of a `match` arm, from its pattern: an arm's sites keep their
/// names when another arm is added, removed or edited.
pub fn arm_key(pattern: &Pattern) -> String {
    let mut key = Key::new();
    key.pattern(pattern);
    key.hex8()
}

/// A written type, as a script writes it: `int`, `Vec3?`, `Map<string, int>`.
pub fn type_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Void => "void".to_owned(),
        TypeRef::Named { name, .. } => name.clone(),
        TypeRef::Optional { inner, .. } => format!("{}?", type_name(inner)),
        TypeRef::Array { element, .. } => format!("{}[]", type_name(element)),
        TypeRef::Generic { name, args, .. } => format!(
            "{name}<{}>",
            args.iter().map(type_name).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// A blake3 hash of a syntax tree, spans excluded.
struct Key(blake3::Hasher);

impl Key {
    fn new() -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"khora.ergon.statement");
        Self(hasher)
    }

    /// The first four bytes, as eight lowercase hex digits.
    fn hex8(&self) -> String {
        self.0.finalize().as_bytes()[..4]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn tag(&mut self, tag: u8) {
        self.0.update(&[tag]);
    }

    /// Length-prefixed, so `ab` + `c` and `a` + `bc` differ.
    fn text(&mut self, text: &str) {
        self.0.update(&(text.len() as u64).to_le_bytes());
        self.0.update(text.as_bytes());
    }

    fn bits(&mut self, bits: u64) {
        self.0.update(&bits.to_le_bytes());
    }

    /// A statement's header, returning its kind.
    fn header(&mut self, statement: &Stmt) -> &'static str {
        match statement {
            Stmt::Let {
                ty, name, value, ..
            } => {
                self.tag(1);
                self.optional_type(ty.as_ref());
                self.text(name);
                self.optional_expr(value.as_ref());
                "let"
            }
            Stmt::Expr(expr) => {
                self.tag(2);
                self.expr(expr);
                "expr"
            }
            Stmt::If { condition, .. } => {
                self.tag(3);
                self.expr(condition);
                "if"
            }
            Stmt::While { condition, .. } => {
                self.tag(4);
                self.expr(condition);
                "while"
            }
            Stmt::For {
                init,
                condition,
                step,
                ..
            } => {
                self.tag(5);
                match init {
                    Some(init) => {
                        self.tag(1);
                        self.header(init);
                    }
                    None => self.tag(0),
                }
                self.optional_expr(condition.as_ref());
                self.optional_expr(step.as_ref());
                "for"
            }
            Stmt::Foreach {
                ty, name, iterable, ..
            } => {
                self.tag(6);
                self.optional_type(ty.as_ref());
                self.text(name);
                self.expr(iterable);
                "foreach"
            }
            Stmt::Return { value, .. } => {
                self.tag(7);
                self.optional_expr(value.as_ref());
                "return"
            }
            Stmt::Break(_) => {
                self.tag(8);
                "break"
            }
            Stmt::Continue(_) => {
                self.tag(9);
                "continue"
            }
            Stmt::Become { state, args, .. } => {
                self.tag(10);
                self.text(state);
                self.exprs(args);
                "become"
            }
            // The subject is the header. Each arm is a branch named by its own
            // pattern (`arm_key`), so an arm added, removed, reordered or
            // re-patterned leaves the sites of the others alone.
            Stmt::Match { subject, .. } => {
                self.tag(11);
                self.expr(subject);
                "match"
            }
            Stmt::Block(_) => {
                self.tag(12);
                "block"
            }
        }
    }

    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Null(_) => self.tag(0),
            Pattern::Binding { ty, name, .. } => {
                self.tag(1);
                self.ty(ty);
                self.text(name);
            }
            Pattern::Wildcard(_) => self.tag(2),
        }
    }

    fn optional_expr(&mut self, expr: Option<&Expr>) {
        match expr {
            Some(expr) => {
                self.tag(1);
                self.expr(expr);
            }
            None => self.tag(0),
        }
    }

    fn optional_type(&mut self, ty: Option<&TypeRef>) {
        match ty {
            Some(ty) => {
                self.tag(1);
                self.ty(ty);
            }
            None => self.tag(0),
        }
    }

    fn ty(&mut self, ty: &TypeRef) {
        self.text(&type_name(ty));
    }

    fn exprs(&mut self, exprs: &[Expr]) {
        self.bits(exprs.len() as u64);
        for expr in exprs {
            self.expr(expr);
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Int { value, .. } => {
                self.tag(1);
                self.bits(*value as u64);
            }
            Expr::Float { value, .. } => {
                self.tag(2);
                self.bits(u64::from(value.to_bits()));
            }
            Expr::Str { value, .. } => {
                self.tag(3);
                self.text(value);
            }
            Expr::Duration { value, .. } => {
                self.tag(4);
                self.bits(u64::from(value.to_bits()));
            }
            Expr::Angle { value, .. } => {
                self.tag(5);
                self.bits(u64::from(value.to_bits()));
            }
            Expr::Bool { value, .. } => {
                self.tag(6);
                self.tag(u8::from(*value));
            }
            Expr::Null(_) => self.tag(7),
            Expr::This(_) => self.tag(8),
            Expr::Ident { name, .. } => {
                self.tag(9);
                self.text(name);
            }
            Expr::ArrayLit { elements, .. } => {
                self.tag(10);
                self.exprs(elements);
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                self.tag(11);
                self.binary(*op);
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Unary { op, operand, .. } => {
                self.tag(12);
                self.tag(match op {
                    UnaryOp::Neg => 0,
                    UnaryOp::Not => 1,
                });
                self.expr(operand);
            }
            Expr::Assign {
                target, op, value, ..
            } => {
                self.tag(13);
                match op {
                    Some(op) => {
                        self.tag(1);
                        self.binary(*op);
                    }
                    None => self.tag(0),
                }
                self.expr(target);
                self.expr(value);
            }
            Expr::Call { callee, args, .. } => {
                self.tag(14);
                self.expr(callee);
                self.exprs(args);
            }
            Expr::Field { object, name, .. } => {
                self.tag(15);
                self.expr(object);
                self.text(name);
            }
            Expr::OptionalField { object, name, .. } => {
                self.tag(16);
                self.expr(object);
                self.text(name);
            }
            Expr::Index { object, index, .. } => {
                self.tag(17);
                self.expr(object);
                self.expr(index);
            }
            Expr::Ternary {
                condition,
                then_value,
                else_value,
                ..
            } => {
                self.tag(18);
                self.expr(condition);
                self.expr(then_value);
                self.expr(else_value);
            }
            Expr::New { ty, args, .. } => {
                self.tag(19);
                self.ty(ty);
                self.exprs(args);
            }
            Expr::Binding { name, value, .. } => {
                self.tag(20);
                self.text(name);
                self.expr(value);
            }
            Expr::Await { operand, .. } => {
                self.tag(21);
                self.expr(operand);
            }
            Expr::Cast { ty, operand, .. } => {
                self.tag(22);
                self.ty(ty);
                self.expr(operand);
            }
        }
    }

    fn binary(&mut self, op: BinaryOp) {
        self.tag(match op {
            BinaryOp::Add => 0,
            BinaryOp::Sub => 1,
            BinaryOp::Mul => 2,
            BinaryOp::Div => 3,
            BinaryOp::Rem => 4,
            BinaryOp::Eq => 5,
            BinaryOp::NotEq => 6,
            BinaryOp::Less => 7,
            BinaryOp::LessEq => 8,
            BinaryOp::Greater => 9,
            BinaryOp::GreaterEq => 10,
            BinaryOp::And => 11,
            BinaryOp::Or => 12,
            BinaryOp::Coalesce => 13,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Span;

    fn int(value: i64, at: u32) -> Expr {
        Expr::Int {
            value,
            span: Span::new(at, at + 1),
        }
    }

    #[test]
    fn a_key_ignores_where_the_statement_was_written() {
        let here = Stmt::Expr(int(1, 0));
        let there = Stmt::Expr(int(1, 40));
        assert_eq!(statement_key(&here), statement_key(&there));
    }

    #[test]
    fn a_key_changes_with_what_the_statement_says() {
        assert_ne!(
            statement_key(&Stmt::Expr(int(1, 0))),
            statement_key(&Stmt::Expr(int(2, 0)))
        );
    }

    #[test]
    fn a_key_is_eight_lowercase_hex_digits() {
        let (kind, key) = statement_key(&Stmt::Expr(int(1, 0)));
        assert_eq!(kind, "expr");
        assert_eq!(key.len(), 8);
        assert!(key
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
