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

//! Structs: their layouts, literals, field reads and `?.`.
//!
//! A struct value is an arena object holding its fields in declaration order,
//! so a field is a position — and the program records each struct's layout,
//! names included, so everything that outlives a frame can match a saved
//! value by field name instead. Its defaults are constants, evaluated here
//! into the layout: a literal leaving a field out, and a saved value missing
//! one, both take them.

use super::keys::type_name;
use super::objects::shape_of_ty;
use super::{Compiler, Shape};
use crate::arena::Owned;
use crate::ast::{Expr, Item, Module, UnaryOp};
use crate::types::Ty;
use crate::vm::{Instruction, Reg, StructLayout, Value};

impl Compiler {
    /// Records every struct the module declares, before any code: a layout,
    /// a field index and a literal all resolve against it.
    pub(super) fn collect_structs(&mut self, module: &Module) {
        for item in &module.items {
            if let Item::Component(decl) = item {
                self.component_decls.insert(decl.name.clone(), decl.clone());
            }
            let Item::Struct(decl) = item else {
                continue;
            };
            self.program.structs.push(StructLayout {
                name: decl.name.clone(),
                fields: decl
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), type_name(&field.ty)))
                    .collect(),
                defaults: decl
                    .fields
                    .iter()
                    .map(|field| field.default.as_ref().and_then(constant))
                    .collect(),
            });
            self.struct_decls.insert(decl.name.clone(), decl.clone());
        }
        // Complete once every struct is known: a default's own struct literal
        // takes the defaults of the fields it leaves out, so a value filled
        // from it is whole wherever it lands.
        let structs = self.program.structs.clone();
        for layout in &mut self.program.structs {
            for (slot, default) in layout.defaults.iter_mut().enumerate() {
                let ty = &layout.fields[slot].1;
                *default = default
                    .take()
                    .and_then(|default| default.completed(ty, &structs));
            }
        }
    }

    /// `name`'s index and its field `field`'s position.
    fn field_of(&self, name: &str, field: &str) -> Option<(u16, u16)> {
        let (layout, declared) = self.program.struct_named(name)?;
        let slot = declared
            .fields
            .iter()
            .position(|(known, _)| known == field)?;
        Some((layout, u16::try_from(slot).ok()?))
    }

    /// The struct `expr` was checked to be, if it is one.
    fn struct_of(&self, expr: &Expr) -> Option<String> {
        match self.type_of(expr)? {
            Ty::Struct(name) => Some(name.clone()),
            Ty::Optional(inner) => match &**inner {
                Ty::Struct(name) => Some(name.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The layout and position `object.field` names, when `object` is a
    /// struct.
    pub(super) fn struct_field(&self, object: &Expr, field: &str) -> Option<(u16, u16)> {
        let name = self.struct_of(object)?;
        self.field_of(&name, field)
    }

    /// `Name { field: value, … }`: each field in declaration order, written or
    /// defaulted, in consecutive registers, then `NewStruct`.
    pub(super) fn compile_struct_lit(
        &mut self,
        name: &str,
        fields: &[(String, crate::diagnostics::Span, Expr)],
        span: crate::diagnostics::Span,
    ) -> (Reg, Shape) {
        let (Some((layout, _)), Some(decl)) = (
            self.program.struct_named(name),
            self.struct_decls.get(name).cloned(),
        ) else {
            self.error(format!("`{name}` is not a struct"), span);
            return self.constant(Value::Unit, Shape::Other);
        };
        let dst = self.registers.temp();
        let mark = self.registers.mark();
        let count = decl.fields.len();
        // The fields written, in the order written: their side effects follow
        // the source, and a frame stopped inside one holds the same values
        // whatever order the struct declares its fields in.
        let written: Vec<(String, Reg)> = fields
            .iter()
            .map(|(field, _, value)| {
                let held = self.registers.temp();
                let inner = self.registers.mark();
                let (register, _) = self.compile_value(value);
                if register != held {
                    self.emit(Instruction::Move {
                        dst: held,
                        src: register,
                    });
                }
                self.registers.release_to(inner);
                (field.clone(), held)
            })
            .collect();
        // Then every field in declaration order, in consecutive registers —
        // after the last written one, where no stop comes between.
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..count).map(|_| self.registers.temp()))
            .collect();
        for (declared, slot) in decl.fields.iter().zip(slots) {
            let inner = self.registers.mark();
            let value = match (
                written.iter().find(|(field, _)| *field == declared.name),
                &declared.default,
            ) {
                (Some((_, register)), _) => *register,
                (None, Some(default)) => self.compile_value(default).0,
                (None, None) => self.zero_of_type(&declared.ty),
            };
            if value != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: value,
                });
            }
            self.registers.release_to(inner);
        }
        self.emit(Instruction::NewStruct {
            dst,
            layout,
            base,
            count: count as u16,
        });
        self.registers.release_to(mark);
        (dst, Shape::Object)
    }

    /// `s.field`, when `s` is a struct.
    pub(super) fn compile_struct_read(
        &mut self,
        whole: &Expr,
        object: &Expr,
        field: &str,
    ) -> Option<(Reg, Shape)> {
        let (layout, slot) = self.struct_field(object, field)?;
        let (source, _) = self.compile_expr(object);
        let dst = self.registers.temp();
        self.emit(Instruction::GetField {
            dst,
            object: source,
            layout,
            slot,
        });
        let shape = self.type_of(whole).map_or(Shape::Other, shape_of_ty);
        Some((dst, shape))
    }

    /// `s?.field`: the field when `s` is present, `null` when it is not.
    pub(super) fn compile_optional_field(
        &mut self,
        whole: &Expr,
        object: &Expr,
        field: &str,
        span: crate::diagnostics::Span,
    ) -> (Reg, Shape) {
        let Some((layout, slot)) = self.struct_field(object, field) else {
            self.error("`?.` reads a struct's field", span);
            return self.constant(Value::Unit, Shape::Other);
        };
        let (source, _) = self.compile_expr(object);
        let dst = self.registers.temp();
        let absent = self.emit(Instruction::JumpIfNull {
            src: source,
            target: usize::MAX,
        });
        self.emit(Instruction::GetField {
            dst,
            object: source,
            layout,
            slot,
        });
        let over = self.emit(Instruction::Jump { target: usize::MAX });
        self.patch_to_here(absent);
        self.emit(Instruction::LoadConst {
            dst,
            value: Value::Null,
        });
        self.patch_to_here(over);
        let shape = self.type_of(whole).map_or(Shape::Other, shape_of_ty);
        (dst, shape)
    }
}

/// The value a constant expression stands for — what a struct field's default
/// is (the checker refused any other).
fn constant(expr: &Expr) -> Option<Owned> {
    Some(match expr {
        Expr::Int { value, .. } => Owned::Scalar(Value::Int(*value)),
        Expr::Float { value, .. } | Expr::Duration { value, .. } | Expr::Angle { value, .. } => {
            Owned::Scalar(Value::Float(*value))
        }
        Expr::Bool { value, .. } => Owned::Scalar(Value::Bool(*value)),
        Expr::Null(_) => Owned::Scalar(Value::Null),
        Expr::Str { value, .. } => Owned::Str(value.clone()),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
            ..
        } => match constant(operand)? {
            Owned::Scalar(Value::Int(value)) => Owned::Scalar(Value::Int(-value)),
            Owned::Scalar(Value::Float(value)) => Owned::Scalar(Value::Float(-value)),
            _ => return None,
        },
        Expr::ArrayLit { elements, .. } => {
            Owned::Array(elements.iter().map(constant).collect::<Option<_>>()?)
        }
        // The fields written; an import fills the rest from the struct's own
        // defaults, as it does for a saved value.
        Expr::StructLit { name, fields, .. } => Owned::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(field, _, value)| Some((field.clone(), constant(value)?)))
                .collect::<Option<_>>()?,
        },
        _ => return None,
    })
}
