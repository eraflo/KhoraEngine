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

//! `foreach (Entity e, Health h, Shield? s) with A without B { … }` — a sweep
//! over the rows a query matched when it started.
//!
//! The rows are taken once, in entity order, into an array held by a hidden
//! local, so a frame stopped mid-sweep — for fuel, by a save, by a reload —
//! carries on from the row it stood on, and a structural change the body
//! makes is a command that leaves the sweep alone. Each row is checked again
//! when it is reached: one an earlier row's body took out of the query is
//! skipped. Its bindings are copies, as `Get` makes them; a system's `ref`
//! copy is written back where the row ends — where `continue` lands, and
//! before a `break` or a `return` leaves the sweep.

use super::keys::type_name;
use super::{shape_of, Compiler, Shape};
use crate::ast::{Block, QueryBinding, TypeRef};
use crate::vm::{Instruction, QueryShape, Reg, SiteKind, Value};

/// What ends a row of a query being compiled: its `ref` copies written back.
#[derive(Debug, Clone)]
pub(super) struct RowWrites {
    /// How many loops are open around the query's body, the query's included
    /// — a `break` at this depth leaves the sweep.
    depth: usize,
    /// The local holding the row's entity.
    entity: Reg,
    /// Each `ref` binding: its copy, the copy as read, its component's name
    /// as a string index, and how many fields it has.
    writes: Vec<(Reg, Reg, u32, u16)>,
}

impl Compiler {
    /// Compiles a query statement.
    pub(super) fn compile_query(
        &mut self,
        bindings: &[QueryBinding],
        with: &[(String, crate::diagnostics::Span)],
        without: &[(String, crate::diagnostics::Span)],
        body: &Block,
    ) {
        let mut shape = QueryShape::default();
        for binding in bindings {
            if let Some((component, optional)) = bound_component(&binding.ty) {
                self.record_read(component);
                if !optional {
                    shape.all.push(component.to_owned());
                }
            }
        }
        for (name, _) in with {
            self.record_read(name);
            shape.all.push(name.clone());
        }
        for (name, _) in without {
            self.record_read(name);
            shape.none.push(name.clone());
        }
        let width = shape.width() as u16;
        let query = self.program.queries.len() as u32;
        self.program.queries.push(shape);

        let scope = self.open_scope();
        let mark = self.registers.mark();
        // Taking the rows is paid for by the candidates it visits: a place to
        // stop before it, when the fuel left cannot.
        self.record_checkpoint("rows");
        let taken = self.registers.temp();
        self.emit(Instruction::QueryRows { dst: taken, query });
        let rows = self.declare_local("#q", Shape::Object, "Entity[]".to_owned());
        self.emit(Instruction::Move {
            dst: rows,
            src: taken,
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
            src: rows,
        });
        let count = self.declare_local("#n", Shape::Int, "int".to_owned());
        self.emit(Instruction::Move {
            dst: count,
            src: length,
        });
        self.registers.release_to(mark);

        // The head: where every row starts, and the back edge lands.
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
        // The bindings first, in a scope of their own apart from the body's
        // statements, as an array `foreach`'s element is: the row is read into
        // them, and no temporary of it may sit in their registers.
        self.naming.branch("bound");
        let locals: Vec<_> = bindings
            .iter()
            .map(|binding| {
                self.declare_local(&binding.name, shape_of(&binding.ty), type_name(&binding.ty))
            })
            .collect();
        // Each `ref` binding's copy as read, and the row's entity, for the
        // write at the row's end.
        let written: Vec<(usize, Reg, &str)> = bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| binding.write)
            .filter_map(|(at, binding)| {
                let (component, _) = bound_component(&binding.ty)?;
                let original = self.declare_local(
                    &format!("#o{}", binding.name),
                    Shape::Object,
                    type_name(&binding.ty),
                );
                Some((at, original, component))
            })
            .collect();
        let row_entity = (!written.is_empty())
            .then(|| self.declare_local("#e", Shape::Other, "Entity".to_owned()));
        self.naming.leave_branch();
        let mark = self.registers.mark();
        let entity = self.registers.temp();
        self.emit(Instruction::GetIndex {
            dst: entity,
            object: rows,
            index: position,
        });
        let holds = self.registers.temp();
        self.emit(Instruction::QueryHolds {
            dst: holds,
            entity,
            query,
            count: width,
        });
        let skip = self.emit(Instruction::JumpIfNot {
            cond: holds,
            target: usize::MAX,
        });
        for (binding, &local) in bindings.iter().zip(&locals) {
            match bound_component(&binding.ty) {
                None => {
                    self.emit(Instruction::Move {
                        dst: local,
                        src: entity,
                    });
                }
                Some((component, _)) => {
                    let component_index = self.intern(component);
                    let fields = self.field_count(component);
                    self.emit(Instruction::GetComponent {
                        dst: local,
                        entity,
                        component: component_index,
                        count: fields,
                    });
                }
            }
        }
        let mut row = RowWrites {
            depth: self.loop_depth(),
            entity: row_entity.unwrap_or(entity),
            writes: Vec::new(),
        };
        for &(at, original, component) in &written {
            let component_index = self.intern(component);
            let fields = self.field_count(component);
            self.emit(Instruction::GetComponent {
                dst: original,
                entity,
                component: component_index,
                count: fields,
            });
            row.writes
                .push((locals[at], original, component_index, fields));
        }
        if let Some(local) = row_entity {
            self.emit(Instruction::Move {
                dst: local,
                src: entity,
            });
        }
        self.registers.release_to(mark);
        self.row_writes.push(row);
        self.compile_statements(&body.statements);
        let context = self.leave_loop_body();
        // Where the row ends: the body's last statement, or a `continue`.
        if let Some(row) = self.row_writes.pop() {
            self.emit_row_writes(&row);
        }
        self.close_scope(inner);
        self.naming.leave_branch();

        // A row the query no longer holds goes straight to the next.
        self.patch_to_here(skip);
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

impl Compiler {
    /// How many fields `component` has.
    fn field_count(&self, component: &str) -> u16 {
        self.component_decls
            .get(component)
            .map_or(0, |decl| decl.fields.len() as u16)
    }

    /// Writes back the `ref` copies of `row`.
    fn emit_row_writes(&mut self, row: &RowWrites) {
        for &(copy, original, component, count) in &row.writes {
            self.emit(Instruction::WriteRow {
                entity: row.entity,
                copy,
                original,
                component,
                count,
            });
        }
    }

    /// Before a `break`: the row of the sweep it leaves, when it leaves one.
    pub(super) fn end_row_for_break(&mut self) {
        let depth = self.loop_depth();
        if let Some(row) = self
            .row_writes
            .last()
            .filter(|row| row.depth == depth)
            .cloned()
        {
            self.emit_row_writes(&row);
        }
    }

    /// Before a `return`: the row of every sweep it leaves, innermost first.
    pub(super) fn end_rows_for_return(&mut self) {
        for row in self.row_writes.clone().iter().rev() {
            self.emit_row_writes(row);
        }
    }
}

/// The component a binding copies, and whether it is optional — `None` for
/// the row's entity.
fn bound_component(ty: &TypeRef) -> Option<(&str, bool)> {
    match ty {
        TypeRef::Named { name, .. } if name == "Entity" => None,
        TypeRef::Named { name, .. } => Some((name, false)),
        TypeRef::Optional { inner, .. } => match &**inner {
            TypeRef::Named { name, .. } => Some((name, true)),
            _ => None,
        },
        _ => None,
    }
}
