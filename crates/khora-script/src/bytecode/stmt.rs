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

//! Compiling statements.
//!
//! # Where break points go
//!
//! Nowhere, explicitly — and that is the point. Fuel is charged per
//! instruction, so *every* instruction is already a place the machine can stop.
//! A loop needs no marker to become interruptible; it is interruptible because
//! its body costs fuel like everything else.
//!
//! What loops do need is a guarantee that they cannot spin without spending,
//! and that follows from the same rule: a back-edge is a `Jump`, a `Jump` costs
//! fuel, so a loop that never terminates still runs out. An infinite loop
//! therefore suspends rather than hanging the frame, and the engine can notice
//! a behavior that never finishes instead of freezing behind it.
//!
//! The compiler's real obligation is narrower: **do not leave live temporaries
//! across a back-edge**. Temporaries are released at the end of every
//! statement, so at a loop's top the live state is locals and a program
//! counter — which is what makes the suspended machine small.

use super::{Compiler, Shape};
use crate::ast::{Block, Expr, Stmt};
use crate::diagnostics::Span;
use crate::vm::{Instruction, Reg, Value};

impl Compiler {
    /// Compiles a block in its own scope.
    pub fn compile_block(&mut self, block: &Block) {
        let scope = self.open_scope();
        for statement in &block.statements {
            self.compile_stmt(statement);
        }
        self.close_scope(scope);
    }

    /// Compiles one statement.
    pub fn compile_stmt(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Let {
                ty, name, value, ..
            } => {
                let mark = self.registers.mark();
                let shape = match (ty, value) {
                    (Some(written), Some(expr)) => {
                        let (source, _) = self.compile_expr(expr);
                        let declared = super::shape_of(written);
                        let slot = self.declare_local(name, declared);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: source,
                        });
                        self.registers.release_to(mark);
                        return;
                    }
                    // `var` takes the initialiser's shape, which is what the
                    // checker inferred its type from.
                    (None, Some(expr)) => {
                        let (source, shape) = self.compile_expr(expr);
                        let slot = self.declare_local(name, shape);
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: source,
                        });
                        self.registers.release_to(mark);
                        return;
                    }
                    (Some(written), None) => super::shape_of(written),
                    (None, None) => Shape::Other,
                };

                // Declared without an initialiser: give it a defined value
                // rather than whatever the register happened to hold.
                let slot = self.declare_local(name, shape);
                self.emit(Instruction::LoadConst {
                    dst: slot,
                    value: Value::Unit,
                });
                self.registers.release_to(mark);
            }

            Stmt::Expr(expr) => {
                let mark = self.registers.mark();
                self.compile_expr(expr);
                // The value is discarded, so its register goes back — this is
                // what keeps a long function from growing a frame per
                // statement.
                self.registers.release_to(mark);
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => self.compile_if(condition, then_branch, else_branch.as_deref()),

            Stmt::While {
                condition, body, ..
            } => self.compile_while(condition, body),

            Stmt::For {
                init,
                condition,
                step,
                body,
                ..
            } => self.compile_for(init.as_deref(), condition.as_ref(), step.as_ref(), body),

            Stmt::Return { value, .. } => {
                let mark = self.registers.mark();
                let src = match value {
                    Some(expr) => self.compile_expr(expr).0,
                    None => {
                        let unit = self.registers.temp();
                        self.emit(Instruction::LoadConst {
                            dst: unit,
                            value: Value::Unit,
                        });
                        unit
                    }
                };
                self.emit(Instruction::Return { src });
                self.registers.release_to(mark);
            }

            Stmt::Block(block) => self.compile_block(block),

            Stmt::Become { state, args, span } => self.compile_become(state, args, *span),

            // `break`, `continue` and `match` need loop context threading or
            // exhaustiveness lowering. Reporting is better than emitting a jump
            // to nowhere.
            other => {
                self.error("this statement cannot be compiled yet", other.span());
            }
        }
    }

    fn compile_if(&mut self, condition: &Expr, then_branch: &Block, else_branch: Option<&Stmt>) {
        let mark = self.registers.mark();
        let (cond, _) = self.compile_expr(condition);
        let to_else = self.emit(Instruction::JumpIfNot {
            cond,
            target: usize::MAX,
        });
        self.registers.release_to(mark);

        self.compile_block(then_branch);

        match else_branch {
            Some(else_branch) => {
                let over_else = self.emit(Instruction::Jump { target: usize::MAX });
                self.patch_to_here(to_else);
                self.compile_stmt(else_branch);
                self.patch_to_here(over_else);
            }
            None => self.patch_to_here(to_else),
        }
    }

    fn compile_while(&mut self, condition: &Expr, body: &Block) {
        // The back-edge lands here. Temporaries are released before it, so a
        // suspension at the top of an iteration captures locals and nothing
        // else.
        let top = self.here();

        let mark = self.registers.mark();
        let (cond, _) = self.compile_expr(condition);
        let exit = self.emit(Instruction::JumpIfNot {
            cond,
            target: usize::MAX,
        });
        self.registers.release_to(mark);

        self.compile_block(body);
        self.emit(Instruction::Jump { target: top });
        self.patch_to_here(exit);
    }

    fn compile_for(
        &mut self,
        init: Option<&Stmt>,
        condition: Option<&Expr>,
        step: Option<&Expr>,
        body: &Block,
    ) {
        // The initialiser's binding is visible to the condition, the step and
        // the body, so the whole loop shares one scope.
        let scope = self.open_scope();
        if let Some(init) = init {
            self.compile_stmt(init);
        }

        let top = self.here();
        let exit = condition.map(|condition| {
            let mark = self.registers.mark();
            let (cond, _) = self.compile_expr(condition);
            let jump = self.emit(Instruction::JumpIfNot {
                cond,
                target: usize::MAX,
            });
            self.registers.release_to(mark);
            jump
        });

        self.compile_block(body);

        if let Some(step) = step {
            let mark = self.registers.mark();
            self.compile_expr(step);
            self.registers.release_to(mark);
        }

        self.emit(Instruction::Jump { target: top });
        if let Some(exit) = exit {
            self.patch_to_here(exit);
        }
        self.close_scope(scope);
    }
}

impl Compiler {
    /// Compiles `become Chase(prey);`.
    ///
    /// The arguments land in consecutive registers before the instruction runs,
    /// the same convention a call uses — and for the same reason: the values are
    /// copied into the state's slots in one pass, so they have to be adjacent
    /// when it starts.
    fn compile_become(&mut self, state: &str, args: &[Expr], span: Span) {
        let Some(layout) = self.behavior.clone() else {
            self.error("`become` is only meaningful inside a behavior", span);
            return;
        };
        let Some(index) = layout.state_index(state) else {
            // The checker has already reported an unknown state; emitting
            // nothing here avoids a second message for one mistake.
            return;
        };

        let mark = self.registers.mark();
        // Every slot reserved before any argument is compiled: compiling one
        // takes temporaries of its own, so reserving as we go would leave the
        // next slot no longer adjacent.
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..args.len()).map(|_| self.registers.temp()))
            .collect();

        for (argument, slot) in args.iter().zip(slots) {
            let inner = self.registers.mark();
            let (value, _) = self.compile_expr(argument);
            if value != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: value,
                });
            }
            self.registers.release_to(inner);
        }

        self.emit(Instruction::Become {
            state: index as u16,
            base,
            argc: args.len() as u8,
            state_slot: layout.state_slot() as u16,
            data_slot: layout.state_data_slot() as u16,
        });
        self.registers.release_to(mark);
    }
}
