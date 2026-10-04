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
//! At the start of every statement and at every loop's head, as a
//! [`Safepoint`](Instruction::Safepoint), each one named
//! ([`sites`](super::sites)). The machine also stops at a function's entry and
//! just past a call's return, which need no marker. Fuel is charged per
//! instruction but checked only there, so a machine stopped for fuel never
//! holds half an expression: at a statement's start every temporary is
//! released and the live state is locals and a program counter.
//!
//! A loop cannot spin without passing one: its back edge lands on its head. An
//! infinite loop therefore still suspends rather than hanging the frame, and
//! the engine can notice a behavior that never finishes instead of freezing
//! behind it.

use super::sites::inferred_type;
use super::{Compiler, Shape};
use crate::ast::{Block, Expr, Stmt};
use crate::diagnostics::Span;
use crate::vm::{Instruction, Reg, SiteKind, Value};

impl Compiler {
    /// Compiles `block` in its own scope, as the branch `branch` of the
    /// statement being compiled.
    pub fn compile_block(&mut self, block: &Block, branch: &str) {
        let scope = self.open_scope();
        self.naming.branch(branch);
        self.compile_statements(&block.statements);
        self.naming.leave_branch();
        self.close_scope(scope);
    }

    /// Compiles sibling statements, each starting at a named safepoint.
    pub fn compile_statements(&mut self, statements: &[Stmt]) {
        self.naming.open_block();
        for statement in statements {
            self.naming.enter(statement);
            // A `while`'s start is its head, which it marks itself: the back
            // edge has to land on it.
            if !matches!(statement, Stmt::While { .. }) {
                self.statement_safepoint(SiteKind::Statement);
            }
            self.compile_stmt(statement);
            self.naming.leave();
        }
        self.naming.close_block();
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
                        let slot =
                            self.declare_local(name, declared, super::keys::type_name(written));
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
                        let slot = self.declare_local(name, shape, inferred_type(shape));
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: source,
                        });
                        self.registers.release_to(mark);
                        return;
                    }
                    (Some(written), None) => {
                        (super::shape_of(written), super::keys::type_name(written))
                    }
                    (None, None) => (Shape::Other, inferred_type(Shape::Other)),
                };

                // Declared without an initialiser: give it a defined value
                // rather than whatever the register happened to hold.
                let slot = self.declare_local(name, shape.0, shape.1);
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

            Stmt::Block(block) => self.compile_block(block, "body"),

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

        self.compile_block(then_branch, "then");

        match else_branch {
            Some(else_branch) => {
                let over_else = self.emit(Instruction::Jump { target: usize::MAX });
                self.patch_to_here(to_else);
                // `else { … }` is the branch's statements; `else if` is one
                // statement of it.
                match else_branch {
                    Stmt::Block(block) => self.compile_block(block, "else"),
                    other => {
                        self.naming.branch("else");
                        self.compile_statements(std::slice::from_ref(other));
                        self.naming.leave_branch();
                    }
                }
                self.patch_to_here(over_else);
            }
            None => self.patch_to_here(to_else),
        }
    }

    fn compile_while(&mut self, condition: &Expr, body: &Block) {
        // The back-edge lands here, on a safepoint that is also the
        // statement's start. Temporaries are released before it, so a
        // suspension at the top of an iteration captures locals and nothing
        // else.
        let top = self.here();
        self.statement_safepoint(SiteKind::LoopHead);

        let mark = self.registers.mark();
        let (cond, _) = self.compile_expr(condition);
        let exit = self.emit(Instruction::JumpIfNot {
            cond,
            target: usize::MAX,
        });
        self.registers.release_to(mark);

        self.compile_block(body, "body");
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

        // The head, after the initialiser: where every iteration starts.
        let top = self.here();
        if let Some(name) = self.naming.point("head") {
            self.record_site(name, SiteKind::LoopHead, Vec::new());
        }
        self.emit(Instruction::Safepoint);
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

        self.compile_block(body, "body");

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

        // Everything else entering the state implies, written before the
        // discriminant moves so the instance is never briefly in a state whose
        // data has not arrived.
        self.emit_state_entry(state);

        self.emit(Instruction::Become {
            state: index as u16,
            base,
            argc: args.len() as u8,
            state_slot: layout.state_slot() as u16,
            data_slot: layout.state_data_slot() as u16,
        });
        self.registers.release_to(mark);
    }

    /// Writes a state's declared defaults and arms its schedules.
    ///
    /// Emitted wherever a state is entered — every `become`, and the initialiser
    /// for the state a fresh instance starts in — because entering is what makes
    /// those true, not the file the state was declared in. Without it `state
    /// Chase { int missed = 7; }` left `missed` unset and every `every` inside a
    /// state resumed a countdown from the last visit.
    ///
    /// Every default is evaluated into a register *before* any of them is
    /// stored, so a default that suspends cannot leave the state half-written.
    pub fn emit_state_entry(&mut self, state: &str) {
        let Some(entry) = self.entries.get(state).cloned() else {
            return;
        };

        let mark = self.registers.mark();
        let mut writes: Vec<(u16, Reg)> = Vec::new();

        for (slot, default, shape) in &entry.fields {
            let register = match default {
                Some(expr) => self.compile_expr(expr).0,
                // Its type's zero rather than unset, for the reason a
                // behavior's field with no written default gets one: `int
                // missed;` reads as a number that starts at nothing.
                None => self.zero_of(*shape),
            };
            writes.push((*slot, register));
        }

        for (slot, seconds) in &entry.timers {
            let register = self.constant(Value::Float(*seconds), Shape::Float).0;
            writes.push((*slot, register));
        }

        for (slot, src) in writes {
            self.emit(Instruction::StoreField { slot, src });
        }
        self.registers.release_to(mark);
    }
}
