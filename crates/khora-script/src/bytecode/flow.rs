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

//! Control flow: branches, loops, and taking an optional apart.
//!
//! Two jumps carry it all — `JumpIfNot` for a condition and `JumpIfNull` for
//! an optional — with absolute targets inside the function, emitted as
//! placeholders and patched once the destination exists.
//!
//! A loop keeps a [`LoopContext`] while its body compiles, so a `break` or a
//! `continue` written anywhere inside it — in an arm, in a narrowed branch —
//! finds the loop it acts on. `continue` lands on a `while`'s head, a safepoint,
//! or on a `for`'s step, from which the back edge does.
//!
//! The narrowing forms — `if (var x = e)`, `while (var x = e)`, a `match` arm
//! binding `T x` — evaluate the value, jump past the branch when it is `null`,
//! and bind the present value as a local of the branch, declared after the
//! value so the local's register cannot be one the value is still using.

use std::collections::HashMap;

use super::keys::{arm_key, type_name};
use super::sites::inferred_type;
use super::{shape_of, Compiler, Shape};
use crate::ast::{Block, Expr, MatchArm, Pattern, Stmt};
use crate::diagnostics::Span;
use crate::vm::{Instruction, Reg, SiteKind};

/// The loop a `break` or `continue` acts on.
#[derive(Debug, Default)]
pub struct LoopContext {
    /// Where `continue` jumps, once known: a `while`'s head is, a `for`'s step
    /// is emitted after the body.
    continue_target: Option<usize>,
    /// `continue` jumps waiting for the step.
    continues: Vec<usize>,
    /// `break` jumps, patched to the first instruction after the loop.
    breaks: Vec<usize>,
}

/// A present value to bind at a branch's start: the local's name, shape and
/// type, and the register the value was evaluated into.
struct Bound<'a> {
    name: &'a str,
    shape: Shape,
    ty: String,
    value: Reg,
}

impl Compiler {
    /// `break;` — to the first instruction after the innermost loop.
    pub(super) fn compile_break(&mut self, span: Span) {
        let jump = self.emit(Instruction::Jump { target: usize::MAX });
        match self.loops.last_mut() {
            Some(context) => context.breaks.push(jump),
            None => self.error("`break` outside a loop reached the compiler", span),
        }
    }

    /// `continue;` — to the innermost loop's next iteration.
    pub(super) fn compile_continue(&mut self, span: Span) {
        let target = match self.loops.last() {
            Some(context) => context.continue_target,
            None => {
                self.error("`continue` outside a loop reached the compiler", span);
                return;
            }
        };
        let jump = self.emit(Instruction::Jump {
            target: target.unwrap_or(usize::MAX),
        });
        if target.is_none() {
            if let Some(context) = self.loops.last_mut() {
                context.continues.push(jump);
            }
        }
    }

    /// Starts a loop's body, `continue` landing on `continue_target` when it
    /// is already known.
    pub(super) fn enter_loop(&mut self, continue_target: Option<usize>) {
        self.loops.push(LoopContext {
            continue_target,
            ..LoopContext::default()
        });
    }

    /// Ends a loop's body: its pending `continue`s land here. The context is
    /// handed back for [`exit_loop`](Self::exit_loop) once the loop's end is
    /// emitted.
    pub(super) fn leave_loop_body(&mut self) -> LoopContext {
        let mut context = self.loops.pop().unwrap_or_default();
        let here = self.here();
        for jump in std::mem::take(&mut context.continues) {
            self.patch(jump, here);
        }
        context
    }

    /// Lands a loop's `break`s here, past its end.
    pub(super) fn exit_loop(&mut self, context: LoopContext) {
        for jump in context.breaks {
            self.patch_to_here(jump);
        }
    }

    /// Evaluates a condition, emitting the jump taken when it fails: an
    /// ordinary one when false, `var x = e` when `e` is `null` — whose present
    /// value is returned to bind in the branch. Temporaries are released
    /// unless a value is to be bound; the caller releases to `mark` then.
    fn compile_test<'a>(&mut self, condition: &'a Expr, mark: usize) -> (usize, Option<Bound<'a>>) {
        if let Expr::Binding { name, value, .. } = condition {
            let (register, shape) = self.compile_expr(value);
            let jump = self.emit(Instruction::JumpIfNull {
                src: register,
                target: usize::MAX,
            });
            let ty = self
                .inferred
                .get(&(std::ptr::from_ref(&**value) as usize))
                .cloned()
                .unwrap_or_else(|| inferred_type(shape));
            let bound = Bound {
                name,
                shape,
                ty,
                value: register,
            };
            return (jump, Some(bound));
        }
        let (cond, _) = self.compile_expr(condition);
        let jump = self.emit(Instruction::JumpIfNot {
            cond,
            target: usize::MAX,
        });
        self.registers.release_to(mark);
        (jump, None)
    }

    /// Compiles `block` as `branch`, with `bound` declared at its start.
    fn compile_bound_block(
        &mut self,
        block: &Block,
        branch: &str,
        bound: Option<Bound<'_>>,
        mark: usize,
    ) {
        self.naming.branch(branch);
        let scope = self.open_scope();
        if let Some(bound) = bound {
            // A scope of its own, apart from the branch's statements': the
            // body may declare a local of the same name over it, and a
            // rebuilt frame must never pair the one with the other.
            self.naming.branch("bound");
            let slot = self.declare_local(bound.name, bound.shape, bound.ty);
            self.naming.leave_branch();
            if slot != bound.value {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: bound.value,
                });
            }
            self.registers.release_to(mark);
        }
        self.compile_statements(&block.statements);
        self.close_scope(scope);
        self.naming.leave_branch();
    }

    /// `if (cond) { … } else …`, and `if (var x = e) { … } else …`.
    pub(super) fn compile_if(
        &mut self,
        condition: &Expr,
        then_branch: &Block,
        else_branch: Option<&Stmt>,
    ) {
        let mark = self.registers.mark();
        let (to_else, bound) = self.compile_test(condition, mark);
        self.compile_bound_block(then_branch, "then", bound, mark);

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

    /// `while (cond) { … }`, and `while (var x = e) { … }`.
    pub(super) fn compile_while(&mut self, condition: &Expr, body: &Block) {
        // The back-edge lands here, on a safepoint that is also the
        // statement's start. Temporaries are released before it, so a
        // suspension at the top of an iteration captures locals and nothing
        // else.
        let top = self.here();
        self.statement_safepoint(SiteKind::LoopHead);

        let mark = self.registers.mark();
        let (exit, bound) = self.compile_test(condition, mark);
        self.enter_loop(Some(top));
        self.compile_bound_block(body, "body", bound, mark);
        let context = self.leave_loop_body();
        self.emit(Instruction::Jump { target: top });
        self.patch_to_here(exit);
        self.exit_loop(context);
    }

    /// `match (subject) { … }` — taking an optional apart.
    ///
    /// The subject is evaluated once. Each arm tests it and runs, or falls to
    /// the next arm's test: `null` when it is null, `T x` when it is present —
    /// binding it — and `_` always. An arm that ran jumps past the rest. The
    /// subject's register is only read by the tests, which run before any
    /// arm's body has, so a body is free to reuse it.
    pub(super) fn compile_match(&mut self, subject: &Expr, arms: &[MatchArm]) {
        let mark = self.registers.mark();
        let (value, _) = self.compile_expr(subject);
        let mut ends: Vec<usize> = Vec::new();
        let mut to_next: Option<usize> = None;
        let mut seen: HashMap<String, usize> = HashMap::new();

        for (index, arm) in arms.iter().enumerate() {
            if let Some(jump) = to_next.take() {
                self.patch_to_here(jump);
            }
            // Named by the pattern, not the arm's position: an arm added above
            // leaves the sites of the others alone.
            let key = format!("arm.{}", arm_key(&arm.pattern));
            let count = seen.entry(key.clone()).or_insert(0);
            let branch = match *count {
                0 => key,
                n => format!("{key}#{n}"),
            };
            *count += 1;

            let bound = match &arm.pattern {
                Pattern::Null(_) => {
                    let to_arm = self.emit(Instruction::JumpIfNull {
                        src: value,
                        target: usize::MAX,
                    });
                    to_next = Some(self.emit(Instruction::Jump { target: usize::MAX }));
                    self.patch_to_here(to_arm);
                    None
                }
                Pattern::Binding { ty, name, .. } => {
                    to_next = Some(self.emit(Instruction::JumpIfNull {
                        src: value,
                        target: usize::MAX,
                    }));
                    Some(Bound {
                        name,
                        shape: shape_of(ty),
                        ty: type_name(ty),
                        value,
                    })
                }
                Pattern::Wildcard(_) => None,
            };
            // Past its tests, an arm no longer needs the subject held: released
            // here for every arm alike, so a site inside one records the same
            // temporaries wherever the arm sits — reordering the arms must not
            // turn a rebuildable frame into a restart.
            self.registers.release_to(mark);
            self.compile_bound_block(&arm.body, &branch, bound, mark);
            if index + 1 < arms.len() {
                ends.push(self.emit(Instruction::Jump { target: usize::MAX }));
            }
        }

        if let Some(jump) = to_next {
            self.patch_to_here(jump);
        }
        for jump in ends {
            self.patch_to_here(jump);
        }
        self.registers.release_to(mark);
    }
}
