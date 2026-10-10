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

//! Operations on an entity: its natives called as methods, and the component
//! writes `Set`, `Add`, `Remove`, `Spawn`.
//!
//! `e.SetPosition(v)` is the same `NativeCall` the engine function always was,
//! the receiver in the first argument register. A component write evaluates
//! the receiver, then each field in the order written, into consecutive
//! registers, and one instruction queues the command — no stop in between.

use super::{Compiler, Shape};
use crate::ast::Expr;
use crate::types::Ty;
use crate::vm::{Instruction, Patch, Reg, Value, WriteMode};

/// A component literal's parts: its name and its written fields.
type Literal<'e> = (&'e str, &'e [(String, crate::diagnostics::Span, Expr)]);

impl Compiler {
    /// `object.name(args)` when `object` is an entity, or `None`.
    pub(super) fn compile_entity_method(
        &mut self,
        object: &Expr,
        name: &str,
        args: &[Expr],
        span: crate::diagnostics::Span,
    ) -> Option<(Reg, Shape)> {
        if self.type_of(object) != Some(&Ty::Entity) {
            return None;
        }
        match name {
            "Set" | "Add" => {
                let mode = if name == "Set" {
                    WriteMode::Set
                } else {
                    WriteMode::Add
                };
                let Some(literal) = args.first().and_then(literal) else {
                    self.error(format!("`{name}` takes a component literal"), span);
                    return Some(self.constant(Value::Unit, Shape::Other));
                };
                Some(self.compile_write(mode, object, literal))
            }
            "Remove" => {
                let Some(Expr::Ident {
                    name: component, ..
                }) = args.first()
                else {
                    self.error("`Remove` takes a component's name", span);
                    return Some(self.constant(Value::Unit, Shape::Other));
                };
                Some(self.compile_remove(object, component))
            }
            _ => {
                let (index, _) = self.natives.get(name).copied()?;
                let all: Vec<&Expr> = std::iter::once(object).chain(args).collect();
                Some(self.emit_call(super::expr::CallTarget::Native(index), name, &all))
            }
        }
    }

    /// `Spawn(position, C1 { … }, …)`.
    pub(super) fn compile_spawn(
        &mut self,
        args: &[Expr],
        span: crate::diagnostics::Span,
    ) -> (Reg, Shape) {
        let Some((position, components)) = args.split_first() else {
            self.error("`Spawn` takes a position", span);
            return self.constant(Value::Unit, Shape::Other);
        };
        let literals: Option<Vec<Literal<'_>>> = components.iter().map(literal).collect();
        let Some(literals) = literals else {
            self.error("`Spawn` takes component literals after its position", span);
            return self.constant(Value::Unit, Shape::Other);
        };

        let mark = self.registers.mark();
        let at = self.registers.temp();
        self.place(position, at);
        let count: usize = literals.iter().map(|(_, fields)| fields.len()).sum();
        let base =
            self.compile_fields(literals.iter().flat_map(|&(component, fields)| {
                fields.iter().map(move |field| (component, field))
            }));
        let patches = literals
            .iter()
            .map(|&(component, fields)| self.component_patch(component, fields))
            .collect();
        self.program.spawns.push(patches);
        let spawn = (self.program.spawns.len() - 1) as u32;
        self.emit(Instruction::SpawnEntity {
            position: at,
            spawn,
            base,
            count: count as u16,
        });
        self.registers.release_to(mark);
        self.constant(Value::Unit, Shape::Other)
    }

    /// `entity.Set(C { … })` and `entity.Add(C { … })`.
    fn compile_write(
        &mut self,
        mode: WriteMode,
        object: &Expr,
        literal: Literal<'_>,
    ) -> (Reg, Shape) {
        let (component, fields) = literal;
        let mark = self.registers.mark();
        let entity = self.registers.temp();
        self.place(object, entity);
        let base = self.compile_fields(fields.iter().map(|field| (component, field)));
        let patch = self.component_patch(component, fields);
        self.emit(Instruction::WriteComponent {
            mode,
            entity,
            patch,
            base,
            count: fields.len() as u16,
        });
        self.registers.release_to(mark);
        self.constant(Value::Unit, Shape::Other)
    }

    /// `entity.Remove(C)`.
    fn compile_remove(&mut self, object: &Expr, component: &str) -> (Reg, Shape) {
        let mark = self.registers.mark();
        let entity = self.registers.temp();
        self.place(object, entity);
        let component = self.intern(component);
        self.emit(Instruction::RemoveComponent { entity, component });
        self.registers.release_to(mark);
        self.constant(Value::Unit, Shape::Other)
    }

    /// The fields' values, in the order written, in consecutive registers;
    /// returns the first. Every slot is reserved before any value is compiled,
    /// so a value's own temporaries cannot land between two of them. An `int`
    /// written to a `float` field arrives as the float it means.
    fn compile_fields<'e>(
        &mut self,
        fields: impl Iterator<Item = (&'e str, &'e (String, crate::diagnostics::Span, Expr))> + Clone,
    ) -> Reg {
        let count = fields.clone().count();
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..count).map(|_| self.registers.temp()))
            .collect();
        for ((component, (field, _, value)), slot) in fields.zip(slots) {
            let inner = self.registers.mark();
            let (register, shape) = self.compile_value(value);
            let declared = self
                .component_decls
                .get(component)
                .and_then(|decl| decl.fields.iter().find(|known| known.name == *field))
                .map(|known| super::shape_of(&known.ty));
            if shape == Shape::Int && declared == Some(Shape::Float) {
                self.emit(Instruction::IntToFloat {
                    dst: slot,
                    src: register,
                });
            } else if register != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: register,
                });
            }
            self.registers.release_to(inner);
        }
        base
    }

    /// `expr` evaluated into `slot`.
    fn place(&mut self, expr: &Expr, slot: Reg) {
        let inner = self.registers.mark();
        let (register, _) = self.compile_expr(expr);
        if register != slot {
            self.emit(Instruction::Move {
                dst: slot,
                src: register,
            });
        }
        self.registers.release_to(inner);
    }

    /// The patch of `component` writing `fields`, shared with every write of
    /// the same fields in the same order.
    fn component_patch(
        &mut self,
        component: &str,
        fields: &[(String, crate::diagnostics::Span, Expr)],
    ) -> u32 {
        let patch = Patch {
            component: component.to_owned(),
            fields: fields.iter().map(|(name, _, _)| name.clone()).collect(),
        };
        let index = match self
            .program
            .patches
            .iter()
            .position(|known| *known == patch)
        {
            Some(index) => index,
            None => {
                self.program.patches.push(patch);
                self.program.patches.len() - 1
            }
        };
        index as u32
    }

    /// The index of `text` among the program's strings — deduplicated: the
    /// same text written in twenty places is one entry.
    pub(super) fn intern(&mut self, text: &str) -> u32 {
        let index = match self.program.strings.iter().position(|known| known == text) {
            Some(index) => index,
            None => {
                self.program.strings.push(text.to_owned());
                self.program.strings.len() - 1
            }
        };
        index as u32
    }
}

/// A component literal's name and fields, when `expr` is one.
fn literal(expr: &Expr) -> Option<Literal<'_>> {
    match expr {
        Expr::StructLit { name, fields, .. } => Some((name.as_str(), fields.as_slice())),
        _ => None,
    }
}
