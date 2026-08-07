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
use crate::diagnostics::Span;
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
            Expr::Str { value, .. } => self.string_constant(value),

            // Read from the host at run time rather than baked in: the same
            // compiled `Guard` runs for a thousand entities, so the subject
            // cannot be a constant in the program.
            Expr::This(_) => {
                let dst = self.registers.temp();
                self.emit(Instruction::LoadSelf { dst });
                (dst, Shape::Other)
            }

            // A local first, then a field. Never the reverse: a parameter named
            // like a field must win inside the member that declared it, which
            // is what every other language with fields does and what an author
            // will expect.
            Expr::Ident { name, span } => match self.lookup_local(name) {
                Some((register, shape)) => (register, shape),
                None => match self.fields.get(name).copied() {
                    Some((slot, shape)) => {
                        let dst = self.registers.temp();
                        self.emit(Instruction::LoadField { dst, slot });
                        (dst, shape)
                    }
                    None => {
                        self.error(format!("`{name}` has no register"), *span);
                        self.constant(Value::Unit, Shape::Other)
                    }
                },
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
                    return match self.fields.get(name).copied() {
                        Some((field, shape)) => self.assign_field(field, shape, *op, value),
                        None => {
                            self.error(format!("`{name}` has no register"), *span);
                            self.constant(Value::Unit, Shape::Other)
                        }
                    };
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

            // `v.x` is a call to the accessor the type declared. Lowered here
            // rather than given its own instruction: a component read is an
            // engine function like any other, and giving it a second mechanism
            // would mean a second thing to keep in step with the declaration.
            Expr::Field { object, name, span } => self.compile_field(object, name, *span),

            // The duration is evaluated *before* suspending, so what is waited
            // for is what the expression meant at the moment `await` was
            // reached — not what it would mean when the machine resumes, by
            // which time the field it read may have changed.
            Expr::Await { operand, .. } => {
                let mark = self.registers.mark();
                let (seconds, _) = self.compile_expr(operand);
                self.registers.release_to(mark);

                let dst = self.registers.temp();
                self.emit(Instruction::Await { seconds, dst });
                (dst, Shape::Other)
            }

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

    /// Writes a behavior field, reading it first for the `+=` forms.
    ///
    /// The read has to be explicit: a field is not in a register, so `health -=
    /// amount` is a load, an arithmetic and a store rather than the single
    /// in-place operation the local case compiles to.
    fn assign_field(
        &mut self,
        slot: u16,
        shape: Shape,
        op: Option<BinaryOp>,
        value: &Expr,
    ) -> (Reg, Shape) {
        let mark = self.registers.mark();

        let src = match op {
            Some(op) => {
                let current = self.registers.temp();
                self.emit(Instruction::LoadField { dst: current, slot });
                let (rhs, rhs_shape) = self.compile_expr(value);
                self.arithmetic(op, current, shape, rhs, rhs_shape)
            }
            None => self.compile_expr(value).0,
        };
        self.emit(Instruction::StoreField { slot, src });

        // The result of an assignment is the value assigned. Reading the field
        // back keeps that true without depending on the temporary surviving the
        // release below.
        self.registers.release_to(mark);
        let dst = self.registers.temp();
        self.emit(Instruction::LoadField { dst, slot });
        (dst, shape)
    }

    /// Loads a constant into a fresh register.
    pub(super) fn constant(&mut self, value: Value, shape: Shape) -> (Reg, Shape) {
        let dst = self.registers.temp();
        self.emit(Instruction::LoadConst { dst, value });
        (dst, shape)
    }

    /// Compiles `v.x` into a call to the accessor `v`'s type declared.
    ///
    /// The receiver's shape is what says *which* accessor, which is why
    /// [`Shape::Engine`] carries the type name: the checker has already proved
    /// the component exists, but the compiler works in shapes and would
    /// otherwise have no way to ask.
    fn compile_field(&mut self, object: &Expr, name: &str, span: Span) -> (Reg, Shape) {
        let mark = self.registers.mark();
        let (receiver, shape) = self.compile_expr(object);

        let Shape::Engine(engine) = shape else {
            // Struct fields and component reads need the ECS bridge to address.
            // The checker reports the ones that are genuinely wrong; this is
            // the shapes it accepts and the compiler cannot yet emit.
            self.error("only an engine type's components can be read yet", span);
            self.registers.release_to(mark);
            return self.constant(Value::Unit, Shape::Other);
        };

        let accessor = crate::native::accessor_name(engine, name);
        let Some((index, result)) = self.natives.get(&accessor).copied() else {
            self.error(format!("`{engine}` has no `{name}`"), span);
            self.registers.release_to(mark);
            return self.constant(Value::Unit, Shape::Other);
        };

        // The accessor takes one argument, so the receiver has to sit in a slot
        // of its own — `NativeCall` reads `base..base + argc`.
        let base = self.registers.temp();
        if receiver != base {
            self.emit(Instruction::Move {
                dst: base,
                src: receiver,
            });
        }
        let dst = self.registers.temp();
        self.emit(Instruction::NativeCall {
            function: index,
            base,
            argc: 1,
            dst,
        });
        (dst, result)
    }

    /// The value a slot of this shape starts at when nothing was written.
    ///
    /// Zero rather than unset, because `int missed;` reads as a number that
    /// starts at nothing — and an unset slot faults on the first arithmetic
    /// instead.
    pub(super) fn zero_of(&mut self, shape: Shape) -> Reg {
        match shape {
            Shape::Int => self.constant(Value::Int(0), Shape::Int).0,
            Shape::Float => self.constant(Value::Float(0.0), Shape::Float).0,
            // The empty string, which the constant table already deduplicates.
            Shape::Str => self.string_constant("").0,
            // An engine type has no zero the compiler may invent: `Vec3::ZERO`
            // is a position, and a rotation's identity is not four zeroes. The
            // slot stays unset, and the first read of it faults rather than
            // silently placing something at the origin.
            Shape::Engine(_) | Shape::Other => self.constant(Value::Unit, Shape::Other).0,
        }
    }

    /// Interns a string literal and loads a reference to it.
    ///
    /// Deduplicated: the same text written in twenty places is one entry. A
    /// literal is immutable, so sharing one is indistinguishable from twenty
    /// copies — except in the size of the program.
    pub(super) fn string_constant(&mut self, text: &str) -> (Reg, Shape) {
        let index = match self.program.strings.iter().position(|known| known == text) {
            Some(index) => index,
            None => {
                self.program.strings.push(text.to_owned());
                self.program.strings.len() - 1
            }
        };

        let dst = self.registers.temp();
        self.emit(Instruction::LoadStr {
            dst,
            index: index as u32,
        });
        (dst, Shape::Str)
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
        // A member calls its siblings by their bare name — `Riposte()`, not
        // `Guard.Riposte()`, which is not even syntax. Resolved first, so a
        // behavior's own method wins over a free function of the same name:
        // inside `Guard`, `Attack()` means the guard's.
        let sibling = self
            .behavior
            .as_ref()
            .map(|layout| crate::dispatch::handler_name(&layout.name, name))
            .and_then(|qualified| self.signatures.get(&qualified).copied());
        if let Some(index) = sibling {
            return self.emit_call(CallTarget::Script(index), name, args);
        }

        // The script's own functions first, matching the checker: whichever it
        // resolved the call against is the one that must be emitted.
        let target = match self.signatures.get(name).copied() {
            Some(index) => CallTarget::Script(index),
            None => match self.natives.get(name).copied() {
                Some((index, _)) => CallTarget::Native(index),
                None => {
                    self.error(format!("`{name}` is not a compiled function"), span);
                    return self.constant(Value::Unit, Shape::Other);
                }
            },
        };

        self.emit_call(target, name, args)
    }

    /// Places the arguments and emits the call.
    ///
    /// Split out because a sibling call resolves differently but is emitted
    /// identically — duplicating the argument placement is exactly how the two
    /// would drift apart.
    fn emit_call(&mut self, target: CallTarget, name: &str, args: &[Expr]) -> (Reg, Shape) {
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
        let argc = args.len() as u8;
        let shape = match target {
            CallTarget::Script(function) => {
                self.emit(Instruction::Call {
                    function,
                    base,
                    argc,
                    dst,
                });
                self.returns.get(name).copied().unwrap_or(Shape::Other)
            }
            CallTarget::Native(function) => {
                self.emit(Instruction::NativeCall {
                    function,
                    base,
                    argc,
                    dst,
                });
                self.natives
                    .get(name)
                    .map(|(_, shape)| *shape)
                    .unwrap_or(Shape::Other)
            }
        };
        (dst, shape)
    }
}

/// Which kind of call a name resolved to.
///
/// The two are indistinguishable in the source and must not be at the point of
/// emission: they address different tables, and a script function's index used
/// as a native's would call whatever sits at that slot in the registry.
#[derive(Debug, Clone, Copy)]
enum CallTarget {
    /// A function compiled from this program.
    Script(usize),
    /// A function the host exposes.
    Native(usize),
}
