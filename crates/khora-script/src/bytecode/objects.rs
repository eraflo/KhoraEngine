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

//! Arrays: literals, indexing, `.Length`, `foreach`, and value semantics.
//!
//! **Value semantics.** A value read from a *place* — a local, a field, an
//! element — and bound to a name is copied, so two names never share one
//! array. A literal or a call's result is fresh, owned by nobody yet, and
//! never copied. A field keeps its own copy anyway: the store exports.
//!
//! **Writes through a path mutate in place**, ordered so that no write goes
//! through a reference taken before a place the run may stop at: the index
//! operands and the right-hand side are evaluated first, then the path is
//! walked and written. A path rooted at a behavior field loads it (a copy),
//! writes into the copy and stores it back — after the right-hand side, so a
//! write another body made to the field while this one waited survives.
//!
//! **Sized operations** — a copy, a field read or write of an array — are
//! checkpoints (see `vm/objects.rs`): each gets a named site, so a frame
//! stopped there before paying for one resumes, and rebuilds, like any other.

use super::keys::type_name;
use super::{Compiler, Shape};
use crate::ast::{Block, Expr, TypeRef};
use crate::diagnostics::Span;
use crate::types::Ty;
use crate::vm::{Instruction, Reg, SiteKind, Value};

/// One step of a path written through: an element, or a struct's field.
enum Step<'a> {
    Index(&'a Expr),
    Field(u16, u16),
}

/// What a path write does at its end: store a value, or combine the
/// element with one (`op=`).
#[derive(Clone, Copy)]
enum Update {
    Set(Reg),
    Combine(crate::ast::BinaryOp, Reg, Shape, Shape),
}

/// A step, its index operand evaluated.
#[derive(Clone, Copy)]
pub(super) enum At {
    Index(Reg),
    Field(u16, u16),
}

/// How many elements an array literal places in registers at a time.
const CHUNK: usize = 16;

/// The shape a checked type compiles to.
pub fn shape_of_ty(ty: &Ty) -> Shape {
    match ty {
        Ty::Int => Shape::Int,
        Ty::Float | Ty::Duration | Ty::Angle => Shape::Float,
        Ty::Str => Shape::Str,
        Ty::Engine(name) => Shape::Engine(name),
        Ty::Array(_) | Ty::Struct(_) => Shape::Object,
        Ty::Optional(inner) => shape_of_ty(inner),
        _ => Shape::Other,
    }
}

/// Whether `expr` produces a value nobody else names — one binding it need
/// not copy.
fn fresh(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::ArrayLit { .. } | Expr::StructLit { .. } | Expr::Call { .. }
    )
}

impl Compiler {
    /// What the checker found `expr` to be.
    pub(super) fn type_of(&self, expr: &Expr) -> Option<&Ty> {
        self.types.get(&(std::ptr::from_ref(expr) as usize))
    }

    /// Names the place a sized operation stands at, with what is live there.
    pub(super) fn record_checkpoint(&mut self, point: &str) {
        if let Some(name) = self.naming.point(point) {
            let temporaries = self.registers.live_temporaries();
            self.record_site(name, SiteKind::Checkpoint, temporaries);
        }
    }

    /// Compiles `expr` as a value to bind: an array read from a place is
    /// copied (value semantics); everything else is as `compile_expr` gives it.
    pub(super) fn compile_value(&mut self, expr: &Expr) -> (Reg, Shape) {
        let (register, shape) = self.compile_expr(expr);
        if shape != Shape::Object || fresh(expr) {
            return (register, shape);
        }
        self.record_checkpoint("copy");
        let dst = self.registers.temp();
        self.emit(Instruction::Copy { dst, src: register });
        (dst, shape)
    }

    /// `[a, b, …]`: built in chunks, so a literal longer than the registers a
    /// frame can lay out still builds — `NewArray` for the first, `Extend` for
    /// the rest.
    pub(super) fn compile_array(&mut self, elements: &[Expr]) -> (Reg, Shape) {
        let dst = self.registers.temp();
        if elements.is_empty() {
            self.emit(Instruction::NewArray {
                dst,
                base: dst,
                count: 0,
            });
            return (dst, Shape::Object);
        }
        for (index, chunk) in elements.chunks(CHUNK).enumerate() {
            let mark = self.registers.mark();
            // Every slot reserved before any element is compiled, as a call's
            // arguments are: compiling one takes temporaries of its own.
            let base = self.registers.temp();
            let slots: Vec<Reg> = std::iter::once(base)
                .chain((1..chunk.len()).map(|_| self.registers.temp()))
                .collect();
            for (element, slot) in chunk.iter().zip(slots) {
                let inner = self.registers.mark();
                let (value, _) = self.compile_value(element);
                if value != slot {
                    self.emit(Instruction::Move {
                        dst: slot,
                        src: value,
                    });
                }
                self.registers.release_to(inner);
            }
            let count = chunk.len() as u16;
            self.emit(match index {
                0 => Instruction::NewArray { dst, base, count },
                _ => Instruction::Extend {
                    array: dst,
                    base,
                    count,
                },
            });
            self.registers.release_to(mark);
        }
        (dst, Shape::Object)
    }

    /// `xs[i]`.
    pub(super) fn compile_index(
        &mut self,
        whole: &Expr,
        object: &Expr,
        index: &Expr,
    ) -> (Reg, Shape) {
        let (array, _) = self.compile_expr(object);
        let (at, _) = self.compile_expr(index);
        let dst = self.registers.temp();
        self.emit(Instruction::GetIndex {
            dst,
            object: array,
            index: at,
        });
        let shape = self.type_of(whole).map_or(Shape::Other, shape_of_ty);
        (dst, shape)
    }

    /// `d.Seconds`, `a.Radians`, `a.Degrees`: a unit read as a plain float.
    /// A duration is held in seconds and an angle in radians, so two of the
    /// three are the value itself.
    pub(super) fn compile_unit_reading(
        &mut self,
        object: &Expr,
        name: &str,
    ) -> Option<(Reg, Shape)> {
        let degrees = match (self.type_of(object)?, name) {
            (Ty::Duration, "Seconds") | (Ty::Angle, "Radians") => false,
            (Ty::Angle, "Degrees") => true,
            _ => return None,
        };
        let (value, _) = self.compile_expr(object);
        if !degrees {
            return Some((value, Shape::Float));
        }
        let (factor, _) = self.constant(Value::Float(180.0 / std::f32::consts::PI), Shape::Float);
        let dst = self.registers.temp();
        self.emit(Instruction::MulFloat {
            dst,
            lhs: value,
            rhs: factor,
        });
        Some((dst, Shape::Float))
    }

    /// `xs.Length` and `s.Length`, when `object` is an array or a string.
    pub(super) fn compile_length(&mut self, object: &Expr, name: &str) -> Option<(Reg, Shape)> {
        let measured = matches!(self.type_of(object), Some(Ty::Array(_) | Ty::Str));
        if name != "Length" || !measured {
            return None;
        }
        let (src, _) = self.compile_expr(object);
        let dst = self.registers.temp();
        self.emit(Instruction::Length { dst, src });
        Some((dst, Shape::Int))
    }

    /// `root[i][j] = v` and `root[i] op= v`.
    pub(super) fn compile_place_assign(
        &mut self,
        target: &Expr,
        op: Option<crate::ast::BinaryOp>,
        value: &Expr,
        span: Span,
    ) -> (Reg, Shape) {
        let Some((name, at)) = self.place_path(target) else {
            self.error("only a variable's elements can be assigned to yet", span);
            return self.constant(Value::Unit, Shape::Other);
        };
        let element = self.type_of(target).map_or(Shape::Other, shape_of_ty);

        // The right-hand side first, whatever the operator: then the path is
        // loaded, read, combined and written back with no stop in between —
        // a write another body makes while this one waits is never erased.
        let update = match op {
            None => Update::Set(self.compile_value(value).0),
            Some(op) => {
                let (rhs, rhs_shape) = self.compile_expr(value);
                Update::Combine(op, rhs, rhs_shape, element)
            }
        };
        let written = self.walk(&name, &at, update);
        (written, element)
    }

    /// The root variable of `target` and its steps, outermost first, with
    /// each index operand evaluated into a register of its own — before
    /// anything else of the write, since what follows may change a local an
    /// index reads. `None` when the path is not rooted at a variable.
    pub(super) fn place_path(&mut self, target: &Expr) -> Option<(String, Vec<At>)> {
        let mut steps: Vec<Step<'_>> = Vec::new();
        let mut root = target;
        loop {
            match root {
                Expr::Index { object, index, .. } => {
                    steps.push(Step::Index(index));
                    root = object;
                }
                Expr::Field { object, name, .. } => match self.struct_field(object, name) {
                    Some((layout, slot)) => {
                        steps.push(Step::Field(layout, slot));
                        root = object;
                    }
                    None => break,
                },
                _ => break,
            }
        }
        steps.reverse();
        let Expr::Ident { name, .. } = root else {
            return None;
        };
        let name = name.clone();
        let at: Vec<At> = steps
            .iter()
            .map(|step| match step {
                Step::Index(index) => {
                    let (register, _) = self.compile_expr(index);
                    let held = self.registers.temp();
                    self.emit(Instruction::Move {
                        dst: held,
                        src: register,
                    });
                    At::Index(held)
                }
                Step::Field(layout, slot) => At::Field(*layout, *slot),
            })
            .collect();
        Some((name, at))
    }

    /// The register a path's root is in: a local's own, or — a behavior
    /// field — a copy loaded for the update, with the field's slot to write
    /// it back to.
    pub(super) fn load_root(&mut self, name: &str) -> (Reg, Option<u16>) {
        match self.lookup_local(name) {
            Some((register, _)) => (register, None),
            None => {
                let slot = self.fields.get(name).map(|(slot, _)| *slot).unwrap_or(0);
                let dst = self.registers.temp();
                self.record_checkpoint("load");
                self.emit(Instruction::LoadField { dst, slot });
                (dst, Some(slot))
            }
        }
    }

    /// Reads one step of a path: an element, or a struct's field.
    pub(super) fn step_into(&mut self, current: Reg, place: At) -> Reg {
        let dst = self.registers.temp();
        self.emit(match place {
            At::Index(index) => Instruction::GetIndex {
                dst,
                object: current,
                index,
            },
            At::Field(layout, slot) => Instruction::GetField {
                dst,
                object: current,
                layout,
                slot,
            },
        });
        dst
    }

    /// Walks `name[at…]` from its root: reads the element, or — `write` —
    /// writes `source` into it, storing a field root back. Returns the
    /// element read, or the root.
    fn walk(&mut self, name: &str, at: &[At], update: Update) -> Reg {
        let (root, field) = self.load_root(name);
        let last = at.len().saturating_sub(1);
        let mut current = root;
        let mut written = current;
        for (step, &place) in at.iter().enumerate() {
            if step == last {
                let src = match update {
                    Update::Set(source) => source,
                    Update::Combine(op, rhs, rhs_shape, shape) => {
                        let old = self.registers.temp();
                        self.emit(match place {
                            At::Index(index) => Instruction::GetIndex {
                                dst: old,
                                object: current,
                                index,
                            },
                            At::Field(layout, slot) => Instruction::GetField {
                                dst: old,
                                object: current,
                                layout,
                                slot,
                            },
                        });
                        self.arithmetic(op, old, shape, rhs, rhs_shape)
                    }
                };
                written = src;
                self.emit(match place {
                    At::Index(index) => Instruction::SetIndex {
                        object: current,
                        index,
                        src,
                    },
                    At::Field(layout, slot) => Instruction::SetField {
                        object: current,
                        layout,
                        slot,
                        src,
                    },
                });
                break;
            }
            let dst = self.registers.temp();
            self.emit(match place {
                At::Index(index) => Instruction::GetIndex {
                    dst,
                    object: current,
                    index,
                },
                At::Field(layout, slot) => Instruction::GetField {
                    dst,
                    object: current,
                    layout,
                    slot,
                },
            });
            current = dst;
        }
        // Written back with the update: no stop between the load and the
        // store, or a write another body made meanwhile would be erased.
        if let Some(slot) = field {
            self.emit(Instruction::WriteBack { slot, src: root });
        }
        written
    }

    /// `foreach (T x in xs) { … }` — over a snapshot of `xs`.
    ///
    /// Three hidden locals, named so no script can spell them, hold the
    /// snapshot, the position and the length, so a frame suspended inside the
    /// body records them like any local. `continue` lands on the step.
    pub(super) fn compile_foreach(
        &mut self,
        written: Option<&TypeRef>,
        name: &str,
        iterable: &Expr,
        body: &Block,
    ) {
        let scope = self.open_scope();
        let sequence = self.type_of(iterable).cloned().unwrap_or(Ty::Error);
        let element_ty = match &sequence {
            Ty::Array(element) => (**element).clone(),
            _ => Ty::Error,
        };
        let element_shape = shape_of_ty(&element_ty);
        let element_name = written.map_or_else(|| element_ty.name(), type_name);

        let mark = self.registers.mark();
        let (source, _) = self.compile_value(iterable);
        let snapshot = self.declare_local("#t", Shape::Object, sequence.name());
        self.emit(Instruction::Move {
            dst: snapshot,
            src: source,
        });
        self.registers.release_to(mark);

        let (zero, _) = self.constant(Value::Int(0), Shape::Int);
        let position = self.declare_local("#i", Shape::Int, "int".to_owned());
        self.emit(Instruction::Move {
            dst: position,
            src: zero,
        });
        let length = self.registers.temp();
        self.emit(Instruction::Length {
            dst: length,
            src: snapshot,
        });
        let count = self.declare_local("#n", Shape::Int, "int".to_owned());
        self.emit(Instruction::Move {
            dst: count,
            src: length,
        });
        self.registers.release_to(mark);

        // The head: where every iteration starts, and the back edge lands.
        let top = self.here();
        if let Some(head) = self.naming.point("head") {
            self.record_site(head, SiteKind::LoopHead, Vec::new());
        }
        self.emit(Instruction::Safepoint);
        let test = self.registers.mark();
        let more = self.registers.temp();
        self.emit(Instruction::Less {
            dst: more,
            lhs: position,
            rhs: count,
        });
        let exit = self.emit(Instruction::JumpIfNot {
            cond: more,
            target: usize::MAX,
        });
        self.registers.release_to(test);

        self.enter_loop(None);
        self.naming.branch("body");
        let inner = self.open_scope();
        let mark = self.registers.mark();
        let item = self.registers.temp();
        self.emit(Instruction::GetIndex {
            dst: item,
            object: snapshot,
            index: position,
        });
        let item = if element_shape == Shape::Object {
            self.record_checkpoint("copy");
            let copy = self.registers.temp();
            self.emit(Instruction::Copy {
                dst: copy,
                src: item,
            });
            copy
        } else {
            item
        };
        // The element in a scope of its own, apart from the body's statements,
        // as a narrowing binding is.
        self.naming.branch("bound");
        let local = self.declare_local(name, element_shape, element_name);
        self.naming.leave_branch();
        self.emit(Instruction::Move {
            dst: local,
            src: item,
        });
        self.registers.release_to(mark);
        self.compile_statements(&body.statements);
        self.close_scope(inner);
        self.naming.leave_branch();
        let context = self.leave_loop_body();

        let step = self.registers.mark();
        let (one, _) = self.constant(Value::Int(1), Shape::Int);
        self.emit(Instruction::AddInt {
            dst: position,
            lhs: position,
            rhs: one,
        });
        self.registers.release_to(step);
        self.emit(Instruction::Jump { target: top });
        self.patch_to_here(exit);
        self.exit_loop(context);
        self.close_scope(scope);
    }
}
