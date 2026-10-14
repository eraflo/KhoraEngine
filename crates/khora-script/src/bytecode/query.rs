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
//! skipped. Its bindings are copies, as `Get` makes them.

use super::keys::type_name;
use super::{shape_of, Compiler, Shape};
use crate::ast::{Block, QueryBinding, TypeRef};
use crate::vm::{Instruction, QueryShape, SiteKind, Value};

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
                    let fields = self
                        .component_decls
                        .get(component)
                        .map_or(0, |decl| decl.fields.len());
                    self.emit(Instruction::GetComponent {
                        dst: local,
                        entity,
                        component: component_index,
                        count: fields as u16,
                    });
                }
            }
        }
        self.registers.release_to(mark);
        self.compile_statements(&body.statements);
        self.close_scope(inner);
        self.naming.leave_branch();
        let context = self.leave_loop_body();

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
