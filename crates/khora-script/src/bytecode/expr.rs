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

//! Compiling expressions.
//!
//! Every expression compiles into a register and reports the [`Shape`] it left
//! there, so the caller knows which arithmetic instruction to reach for. The
//! shape is threaded upward rather than recomputed, which keeps the compiler
//! from re-deriving what the type checker already proved.

use super::{Compiler, Shape};
use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::vm::{Instruction, Reg, Value};

impl Compiler {
    /// Compiles `expr` into a fresh register.
    pub fn compile_expr(&mut self, expr: &Expr) -> (Reg, Shape) {
        match expr {
            Expr::Int { value, .. } => self.constant(Value::Int(*value), Shape::Int),
            Expr::Float { value, .. } => self.constant(Value::Float(*value), Shape::Float),
            // Durations and angles are already normalised to seconds and
            // radians by the lexer, so they are plain floats from here on.
            Expr::Duration { value, .. } | Expr::Angle { value, .. } => {
                self.constant(Value::Float(*value), Shape::Float)
            }
            Expr::Bool { value, .. } => self.constant(Value::Bool(*value), Shape::Other),
            Expr::Null(_) => self.constant(Value::Null, Shape::Other),

            Expr::Ident { name, span } => match self.lookup_local(name) {
                Some((register, shape)) => (register, shape),
                None => {
                    self.error(format!("`{name}` has no register"), *span);
                    self.constant(Value::Unit, Shape::Other)
                }
            },

            Expr::Binary { op, lhs, rhs, .. } => self.compile_binary(*op, lhs, rhs),
            Expr::Unary { op, operand, .. } => self.compile_unary(*op, operand),
            Expr::Assign {
                target,
                op,
                value,
                span,
            } => {
                let Expr::Ident { name, .. } = &**target else {
                    // Fields and elements need the ECS bridge to address; until
                    // then, refusing is better than emitting a write to nowhere.
                    self.error("only variables can be assigned to yet", *span);
                    return self.constant(Value::Unit, Shape::Other);
                };
                let Some((slot, shape)) = self.lookup_local(name) else {
                    self.error(format!("`{name}` has no register"), *span);
                    return self.constant(Value::Unit, Shape::Other);
                };

                match op {
                    // `a += b` is `a = a + b`, so it compiles to exactly that.
                    Some(op) => {
                        let mark = self.registers.mark();
                        let (rhs, rhs_shape) = self.compile_expr(value);
                        let result = self.arithmetic(*op, slot, shape, rhs, rhs_shape);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: result,
                        });
                        self.registers.release_to(mark);
                    }
                    None => {
                        let mark = self.registers.mark();
                        let (source, _) = self.compile_expr(value);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: source,
                        });
                        self.registers.release_to(mark);
                    }
                }
                (slot, shape)
            }

            Expr::Call { callee, args, span } => self.compile_call(callee, args, *span),

            Expr::Cast { ty, operand, .. } => {
                // int and float share a register representation, and the VM
                // widens an int wherever a float is read. The cast therefore
                // only changes what the compiler believes about the value.
                let (register, _) = self.compile_expr(operand);
                (register, super::shape_of(ty))
            }

            Expr::Ternary {
                condition,
                then_value,
                else_value,
                ..
            } => self.compile_ternary(condition, then_value, else_value),

            // Everything else needs the engine bridge to mean anything. A
            // placeholder keeps the program well-formed; the checker has
            // already refused the cases that are genuinely wrong.
            other => {
                self.error("this expression cannot be compiled yet", other.span());
                self.constant(Value::Unit, Shape::Other)
            }
        }
    }

    /// Loads a constant into a fresh register.
    fn constant(&mut self, value: Value, shape: Shape) -> (Reg, Shape) {
        let dst = self.registers.temp();
        self.emit(Instruction::LoadConst { dst, value });
        (dst, shape)
    }

    fn compile_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> (Reg, Shape) {
        match op {
            // Short-circuiting cannot be expressed as two evaluated operands,
            // so it compiles to branches rather than to an instruction.
            BinaryOp::And | BinaryOp::Or => self.compile_short_circuit(op, lhs, rhs),
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
                    _ => {
                        let register = self.arithmetic(op, left, left_shape, right, right_shape);
                        let shape = if left_shape == Shape::Float || right_shape == Shape::Float {
                            Shape::Float
                        } else {
                            left_shape
                        };
                        (register, shape)
                    }
                }
            }
        }
    }

    /// Emits the arithmetic instruction for these operand shapes.
    ///
    /// A mixed pair compiles to the float form: the VM widens an integer when
    /// it reads a float, so no conversion instruction is needed.
    pub fn arithmetic(
        &mut self,
        op: BinaryOp,
        lhs: Reg,
        lhs_shape: Shape,
        rhs: Reg,
        rhs_shape: Shape,
    ) -> Reg {
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
        dst
    }

    fn compile_unary(&mut self, op: UnaryOp, operand: &Expr) -> (Reg, Shape) {
        let (src, shape) = self.compile_expr(operand);
        let dst = self.registers.temp();
        match op {
            UnaryOp::Not => {
                self.emit(Instruction::Not { dst, src });
                (dst, Shape::Other)
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

    /// `&&` and `||`, which must not evaluate their right operand needlessly.
    fn compile_short_circuit(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> (Reg, Shape) {
        let dst = self.registers.temp();
        let (left, _) = self.compile_expr(lhs);
        self.emit(Instruction::Move { dst, src: left });

        // For `&&` skip the right operand when the left is false; for `||`,
        // when it is true — which is the same test against a negated copy.
        let test = if op == BinaryOp::Or {
            let negated = self.registers.temp();
            self.emit(Instruction::Not {
                dst: negated,
                src: dst,
            });
            negated
        } else {
            dst
        };

        let skip = self.emit(Instruction::JumpIfNot {
            cond: test,
            target: usize::MAX,
        });

        let (right, _) = self.compile_expr(rhs);
        self.emit(Instruction::Move { dst, src: right });
        self.patch_to_here(skip);

        (dst, Shape::Other)
    }

    fn compile_ternary(
        &mut self,
        condition: &Expr,
        then_value: &Expr,
        else_value: &Expr,
    ) -> (Reg, Shape) {
        let dst = self.registers.temp();
        let (cond, _) = self.compile_expr(condition);

        let to_else = self.emit(Instruction::JumpIfNot {
            cond,
            target: usize::MAX,
        });

        let (then_reg, then_shape) = self.compile_expr(then_value);
        self.emit(Instruction::Move { dst, src: then_reg });
        let over_else = self.emit(Instruction::Jump { target: usize::MAX });

        self.patch_to_here(to_else);
        let (else_reg, else_shape) = self.compile_expr(else_value);
        self.emit(Instruction::Move { dst, src: else_reg });
        self.patch_to_here(over_else);

        // Both branches were proved compatible; a float on either side makes
        // the result a float.
        let shape = if then_shape == Shape::Float || else_shape == Shape::Float {
            Shape::Float
        } else {
            then_shape
        };
        (dst, shape)
    }

    fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: crate::diagnostics::Span,
    ) -> (Reg, Shape) {
        let Expr::Ident { name, .. } = callee else {
            self.error("only named functions can be called yet", span);
            return self.constant(Value::Unit, Shape::Other);
        };
        let Some(index) = self.signatures.get(name).copied() else {
            self.error(format!("`{name}` is not a compiled function"), span);
            return self.constant(Value::Unit, Shape::Other);
        };

        // Arguments must land in *consecutive* registers: the callee's frame
        // starts at the first of them, so its parameters need no copying.
        //
        // Every slot is reserved before any argument is compiled. Reserving as
        // we go would not work: compiling an argument takes temporaries of its
        // own, so the next slot would no longer be adjacent.
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..args.len()).map(|_| self.registers.temp()))
            .collect();

        for (argument, slot) in args.iter().zip(slots) {
            let mark = self.registers.mark();
            let (value, _) = self.compile_expr(argument);
            if value != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: value,
                });
            }
            // The argument's own temporaries are done with; the slot itself
            // sits below the mark and survives.
            self.registers.release_to(mark);
        }

        let dst = self.registers.temp();
        self.emit(Instruction::Call {
            function: index,
            base,
            argc: args.len() as u8,
            dst,
        });

        let shape = self.returns.get(name).copied().unwrap_or(Shape::Other);
        (dst, shape)
    }
}
