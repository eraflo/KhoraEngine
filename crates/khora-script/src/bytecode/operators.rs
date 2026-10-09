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

//! Operators: arithmetic, comparison, `??`, unary minus and `!`.
//!
//! On numbers an operator is an instruction picked by the operands' shapes. On
//! an engine type it is a call to the engine function the checker found for it
//! (`"Vec3 + Vec3"`, see [`native::operators`](crate::native::operators)).

use super::{Compiler, Shape};
use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::vm::{Instruction, Reg};

impl Compiler {
    pub(super) fn compile_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> (Reg, Shape) {
        match op {
            // Short-circuiting cannot be expressed as two evaluated operands,
            // so it compiles to branches rather than to an instruction.
            BinaryOp::And | BinaryOp::Or => self.compile_short_circuit(op, lhs, rhs),
            BinaryOp::Coalesce => self.compile_coalesce(lhs, rhs),
            _ => {
                let (left, left_shape) = self.compile_expr(lhs);
                let (right, right_shape) = self.compile_expr(rhs);
                match op {
                    BinaryOp::Eq | BinaryOp::NotEq => {
                        let dst = self.registers.temp();
                        self.emit(Instruction::Eq {
                            dst,
                            lhs: left,
                            rhs: right,
                        });
                        if op == BinaryOp::NotEq {
                            self.emit(Instruction::Not { dst, src: dst });
                        }
                        (dst, Shape::Other)
                    }
                    BinaryOp::Less | BinaryOp::LessEq => {
                        let dst = self.registers.temp();
                        let instruction = if op == BinaryOp::Less {
                            Instruction::Less {
                                dst,
                                lhs: left,
                                rhs: right,
                            }
                        } else {
                            Instruction::LessEq {
                                dst,
                                lhs: left,
                                rhs: right,
                            }
                        };
                        self.emit(instruction);
                        (dst, Shape::Other)
                    }
                    // `a > b` is `b < a`. Emitting the mirrored form keeps two
                    // instructions out of the set for no loss.
                    BinaryOp::Greater | BinaryOp::GreaterEq => {
                        let dst = self.registers.temp();
                        let instruction = if op == BinaryOp::Greater {
                            Instruction::Less {
                                dst,
                                lhs: right,
                                rhs: left,
                            }
                        } else {
                            Instruction::LessEq {
                                dst,
                                lhs: right,
                                rhs: left,
                            }
                        };
                        self.emit(instruction);
                        (dst, Shape::Other)
                    }
                    // `"a" + "b"` is not an addition. The checker has already
                    // proved both sides are text; joining them allocates, so it
                    // gets its own instruction rather than a numeric one that
                    // would fault at run time.
                    BinaryOp::Add if left_shape == Shape::Str || right_shape == Shape::Str => {
                        let dst = self.registers.temp();
                        self.emit(Instruction::Concat {
                            dst,
                            lhs: left,
                            rhs: right,
                        });
                        (dst, Shape::Str)
                    }
                    _ => self.arithmetic_shaped(op, left, left_shape, right, right_shape),
                }
            }
        }
    }

    /// Emits the arithmetic for these operand shapes, returning its register.
    pub fn arithmetic(
        &mut self,
        op: BinaryOp,
        lhs: Reg,
        lhs_shape: Shape,
        rhs: Reg,
        rhs_shape: Shape,
    ) -> Reg {
        self.arithmetic_shaped(op, lhs, lhs_shape, rhs, rhs_shape).0
    }

    /// Emits the arithmetic for these operand shapes, returning its register
    /// and shape.
    ///
    /// On an engine type it is a call to the operation's engine function —
    /// `"Vec3 + Vec3"` — which the checker has proved exists. On numbers, a
    /// mixed pair compiles to the float form: the VM widens an integer when it
    /// reads a float, so no conversion instruction is needed.
    pub fn arithmetic_shaped(
        &mut self,
        op: BinaryOp,
        lhs: Reg,
        lhs_shape: Shape,
        rhs: Reg,
        rhs_shape: Shape,
    ) -> (Reg, Shape) {
        if matches!(lhs_shape, Shape::Engine(_)) || matches!(rhs_shape, Shape::Engine(_)) {
            let name = crate::native::operators::binary_operator(
                operand_name(lhs_shape),
                operator_symbol(op),
                operand_name(rhs_shape),
            );
            return self.call_operator(&name, &[lhs, rhs], lhs);
        }

        let dst = self.registers.temp();
        let float = lhs_shape == Shape::Float || rhs_shape == Shape::Float;

        let instruction = match (op, float) {
            (BinaryOp::Add, false) => Instruction::AddInt { dst, lhs, rhs },
            (BinaryOp::Add, true) => Instruction::AddFloat { dst, lhs, rhs },
            (BinaryOp::Sub, false) => Instruction::SubInt { dst, lhs, rhs },
            (BinaryOp::Sub, true) => Instruction::SubFloat { dst, lhs, rhs },
            (BinaryOp::Mul, false) => Instruction::MulInt { dst, lhs, rhs },
            (BinaryOp::Mul, true) => Instruction::MulFloat { dst, lhs, rhs },
            (BinaryOp::Div, false) => Instruction::DivInt { dst, lhs, rhs },
            (BinaryOp::Div, true) => Instruction::DivFloat { dst, lhs, rhs },
            (BinaryOp::Rem, _) => Instruction::RemInt { dst, lhs, rhs },
            // The checker rejects every other operator here, so reaching this
            // arm means a compiler bug rather than a bad program. Emitting a
            // move keeps the output well-formed.
            _ => Instruction::Move { dst, src: lhs },
        };
        self.emit(instruction);
        let shape = if float { Shape::Float } else { lhs_shape };
        (dst, shape)
    }

    /// A call to the engine-type operation `name` on `operands`, placed in
    /// consecutive registers as every call's arguments are.
    ///
    /// The checker refused any operation the registry lacks, so a missing one
    /// here means the compiler lost an operand's shape: it is reported as an
    /// error rather than compiled into something that runs and is wrong.
    fn call_operator(&mut self, name: &str, operands: &[Reg], fallback: Reg) -> (Reg, Shape) {
        let Some((function, shape)) = self.natives.get(name).copied() else {
            self.error(
                format!("the compiler could not resolve the operation `{name}`"),
                crate::diagnostics::Span::empty(0),
            );
            return (fallback, Shape::Other);
        };
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..operands.len()).map(|_| self.registers.temp()))
            .collect();
        for (&operand, &slot) in operands.iter().zip(&slots) {
            self.emit(Instruction::Move {
                dst: slot,
                src: operand,
            });
        }
        let dst = self.registers.temp();
        self.emit(Instruction::NativeCall {
            function,
            base,
            argc: operands.len() as u8,
            dst,
        });
        (dst, shape)
    }

    /// `a ?? b`: `a`, unless it is null — and only then is `b` evaluated.
    ///
    /// The result has the optional's shape: a `float?` falls back to a float
    /// even when the fallback is written as an int, which is widened here so
    /// the value is what the checker said it is.
    fn compile_coalesce(&mut self, lhs: &Expr, rhs: &Expr) -> (Reg, Shape) {
        let dst = self.registers.temp();
        let mark = self.registers.mark();
        let (left, left_shape) = self.compile_expr(lhs);
        self.emit(Instruction::Move { dst, src: left });
        let null = self.registers.temp();
        self.emit(Instruction::IsNull {
            dst: null,
            src: dst,
        });
        let present = self.emit(Instruction::JumpIfNot {
            cond: null,
            target: usize::MAX,
        });
        self.registers.release_to(mark);

        let (right, right_shape) = self.compile_expr(rhs);
        let shape = match (left_shape, right_shape) {
            (Shape::Other, other) => other,
            (Shape::Float, _) | (_, Shape::Float) => Shape::Float,
            (known, _) => known,
        };
        if shape == Shape::Float && right_shape == Shape::Int {
            self.emit(Instruction::IntToFloat { dst, src: right });
        } else {
            self.emit(Instruction::Move { dst, src: right });
        }
        self.registers.release_to(mark);
        self.patch_to_here(present);
        (dst, shape)
    }

    pub(super) fn compile_unary(&mut self, op: UnaryOp, operand: &Expr) -> (Reg, Shape) {
        let (src, shape) = self.compile_expr(operand);
        let dst = self.registers.temp();
        match op {
            UnaryOp::Not => {
                self.emit(Instruction::Not { dst, src });
                (dst, Shape::Other)
            }
            UnaryOp::Neg if matches!(shape, Shape::Engine(_)) => {
                let name = crate::native::operators::negation(operand_name(shape));
                self.call_operator(&name, &[src], src)
            }
            UnaryOp::Neg => {
                let instruction = if shape == Shape::Float {
                    Instruction::NegFloat { dst, src }
                } else {
                    Instruction::NegInt { dst, src }
                };
                self.emit(instruction);
                (dst, shape)
            }
        }
    }
}

/// How an operand is named in an operator's engine function: an engine type by
/// its name, a number as `float`.
fn operand_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Engine(name) => name,
        Shape::Int | Shape::Float => "float",
        Shape::Str => "string",
        Shape::Object | Shape::Other => "?",
    }
}

/// An arithmetic operator as a script writes it.
fn operator_symbol(op: BinaryOp) -> &'static str {
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
