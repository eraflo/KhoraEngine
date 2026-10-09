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

//! The one form a value takes outside the frame arena.
//!
//! Three places hold a value past its frame: a behavior's persistent store, a
//! suspended machine, and — through the bridge — a scene or a save. Each needs
//! the same thing: a value with no frame reference inside it. [`Owned`] is that
//! value, a tree, and [`Arena::export`] / [`Arena::import`] are the only two
//! ways across the boundary — so the three cannot disagree about what crossing
//! it means.
//!
//! Inside the arena a value is slot-ordered and referenced; outside it is
//! owned, and a struct keeps its field *names*: an import matches them against
//! the program it goes into, so a value survives an edit that adds, removes or
//! reorders a field — an added one takes its default, or its type's zero.

use serde::{Deserialize, Serialize};

use super::{Arena, ArenaError, Object};
use crate::vm::{ObjRef, Program, StrRef, StructLayout, Value};

/// A value that owns everything it holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Owned {
    /// A value that is its own content — never text or an object.
    Scalar(Value),
    /// Text.
    Str(String),
    /// An array's elements.
    Array(Vec<Owned>),
    /// A struct: its name and its fields by name, in declaration order.
    Struct {
        /// The struct's name.
        name: String,
        /// Each field and its value.
        fields: Vec<(String, Owned)>,
    },
}

impl Owned {
    /// How many values it holds, nested ones included — what copying it costs.
    pub fn size(&self) -> usize {
        match self {
            Self::Scalar(_) | Self::Str(_) => 1,
            Self::Array(items) => 1 + items.iter().map(Owned::size).sum::<usize>(),
            Self::Struct { fields, .. } => {
                1 + fields.iter().map(|(_, value)| value.size()).sum::<usize>()
            }
        }
    }

    /// Whether it is an array or a struct — what a sized operation is charged
    /// for.
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Array(_) | Self::Struct { .. })
    }

    /// Whether it can be held by a slot declared `ty` (as written), all the
    /// way down: a struct by its name and each field by its declared type.
    /// `Unit` is "nothing written" and fits anything; an empty `ty` (a layout
    /// recorded before types were) takes anything on trust.
    pub fn fits(&self, ty: &str, structs: &[StructLayout]) -> bool {
        if ty.is_empty() || matches!(self, Self::Scalar(Value::Unit)) {
            return true;
        }
        if let Some(inner) = ty.strip_suffix('?') {
            return matches!(self, Self::Scalar(Value::Null)) || self.fits(inner, structs);
        }
        if let Some(element) = ty.strip_suffix("[]") {
            return matches!(self, Self::Array(items) if items.iter().all(|item| item.fits(element, structs)));
        }
        match (ty, self) {
            ("int", Self::Scalar(Value::Int(_))) => true,
            ("float" | "Duration" | "Angle", Self::Scalar(Value::Float(_) | Value::Int(_))) => true,
            ("bool", Self::Scalar(Value::Bool(_))) => true,
            ("string", Self::Str(_)) => true,
            ("Entity", Self::Scalar(Value::Entity(_))) => true,
            ("Vec2", Self::Scalar(Value::Vec2(_))) => true,
            ("Vec3", Self::Scalar(Value::Vec3(_))) => true,
            ("Vec4", Self::Scalar(Value::Vec4(_))) => true,
            ("Quat", Self::Scalar(Value::Quat(_))) => true,
            ("Color", Self::Scalar(Value::Color(_))) => true,
            (_, Self::Struct { name, fields }) if name == ty => {
                let Some(layout) = structs.iter().find(|layout| layout.name == *name) else {
                    return false;
                };
                // A field the struct no longer declares is dropped on the way
                // in; every other one must fit what it is declared now.
                fields.iter().all(|(field, value)| {
                    layout
                        .fields
                        .iter()
                        .find(|(declared, _)| declared == field)
                        .is_none_or(|(_, declared)| value.fits(declared, structs))
                })
            }
            _ => false,
        }
    }

    /// The struct with every field present: a field it lacks takes its
    /// declared default, else its type's zero — `None` when one has neither.
    /// Fields of struct type are completed in turn. Anything else is itself.
    pub fn completed(&self, ty: &str, structs: &[StructLayout]) -> Option<Owned> {
        self.completed_within(ty, structs, 0)
    }

    fn completed_within(&self, ty: &str, structs: &[StructLayout], depth: usize) -> Option<Owned> {
        // Defaults that build themselves are refused by the checker; this
        // bound only keeps a program that was refused from looping.
        if depth > 64 {
            return None;
        }
        let ty = ty.trim_end_matches('?');
        if let (Some(element), Self::Array(items)) = (ty.strip_suffix("[]"), self) {
            return Some(Self::Array(
                items
                    .iter()
                    .map(|item| item.completed_within(element, structs, depth + 1))
                    .collect::<Option<_>>()?,
            ));
        }
        let Self::Struct { name, fields } = self else {
            return Some(self.clone());
        };
        let layout = structs.iter().find(|layout| layout.name == *name)?;
        let fields = layout
            .fields
            .iter()
            .enumerate()
            .map(|(slot, (field, field_ty))| {
                let value = match fields.iter().find(|(saved, _)| saved == field) {
                    Some((_, value)) => value.clone(),
                    None => match layout.defaults.get(slot) {
                        Some(Some(default)) => default.clone(),
                        _ => Owned::zero(field_ty)?,
                    },
                };
                Some((
                    field.clone(),
                    value.completed_within(field_ty, structs, depth + 1)?,
                ))
            })
            .collect::<Option<_>>()?;
        Some(Self::Struct {
            name: name.clone(),
            fields,
        })
    }

    /// The value a field of type `ty` (as written) starts at when nothing was
    /// written: its zero, or `None` for a type that has none.
    pub fn zero(ty: &str) -> Option<Owned> {
        if ty.ends_with('?') {
            return Some(Self::Scalar(Value::Null));
        }
        if ty.ends_with("[]") {
            return Some(Self::Array(Vec::new()));
        }
        Some(match ty {
            "int" => Self::Scalar(Value::Int(0)),
            "float" | "Duration" | "Angle" => Self::Scalar(Value::Float(0.0)),
            "bool" => Self::Scalar(Value::Bool(false)),
            "string" => Self::Str(String::new()),
            _ => return None,
        })
    }
}

impl Arena {
    /// A deep copy of `value` out of the arena: text and objects by value,
    /// literals and struct names resolved against `program`.
    ///
    /// Fails on a reference that names nothing — stale, or held by a machine.
    pub fn export(&self, value: Value, program: &Program) -> Result<Owned, ArenaError> {
        Ok(match value {
            Value::Str(StrRef::Const(index)) => Owned::Str(
                program
                    .strings
                    .get(index as usize)
                    .cloned()
                    .ok_or(ArenaError::OutOfBounds)?,
            ),
            Value::Str(StrRef::Arena(handle)) | Value::Obj(ObjRef::Arena(handle)) => {
                match self.get(handle)? {
                    Object::Str(text) => Owned::Str(text.clone()),
                    Object::Array(items) => Owned::Array(
                        items
                            .iter()
                            .map(|item| self.export(*item, program))
                            .collect::<Result<_, _>>()?,
                    ),
                    Object::Struct { layout, fields } => {
                        let layout = program
                            .structs
                            .get(usize::from(*layout))
                            .ok_or(ArenaError::Mismatch)?;
                        Owned::Struct {
                            name: layout.name.clone(),
                            fields: layout
                                .fields
                                .iter()
                                .zip(fields)
                                .map(|((name, _), value)| {
                                    Ok((name.clone(), self.export(*value, program)?))
                                })
                                .collect::<Result<_, ArenaError>>()?,
                        }
                    }
                }
            }
            Value::Str(StrRef::Held(_)) | Value::Obj(ObjRef::Held(_)) => {
                return Err(ArenaError::OutOfBounds)
            }
            scalar => Owned::Scalar(scalar),
        })
    }

    /// `owned`, copied into the arena. A struct is matched to `program`'s
    /// declaration by field name: a field it lacks takes the declared default,
    /// else its type's zero, else the value does not fit
    /// ([`ArenaError::Mismatch`]); a field `program` no longer declares is
    /// dropped.
    pub fn import(&mut self, owned: &Owned, program: &Program) -> Result<Value, ArenaError> {
        Ok(match owned {
            Owned::Scalar(value) => *value,
            Owned::Str(text) => Value::Str(StrRef::Arena(self.alloc(Object::Str(text.clone()))?)),
            Owned::Array(items) => {
                let values = items
                    .iter()
                    .map(|item| self.import(item, program))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Obj(ObjRef::Arena(self.alloc(Object::Array(values))?))
            }
            Owned::Struct { name, fields } => {
                let (index, layout) = program.struct_named(name).ok_or(ArenaError::Mismatch)?;
                for (field, _) in fields {
                    if !layout.fields.iter().any(|(declared, _)| declared == field) {
                        log::warn!("`{name}` no longer declares `{field}`: its value is dropped");
                    }
                }
                let values = layout
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(slot, (field, ty))| {
                        let saved = fields.iter().find(|(name, _)| name == field);
                        match saved {
                            // A field retyped since it was saved does not
                            // take the old kind.
                            Some((_, value)) if !value.fits(ty, &program.structs) => {
                                Err(ArenaError::Mismatch)
                            }
                            Some((_, value)) => self.import(value, program),
                            None => {
                                let fill = filling(layout, slot, ty).ok_or(ArenaError::Mismatch)?;
                                self.import(&fill, program)
                            }
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Obj(ObjRef::Arena(self.alloc(Object::Struct {
                    layout: index,
                    fields: values,
                })?))
            }
        })
    }

    /// Whether `value` holds exactly `owned`, compared in place: a resume in
    /// the frame that suspended finds what it held still there.
    pub fn matches(&self, value: Value, owned: &Owned, program: &Program) -> bool {
        match (value, owned) {
            (Value::Str(StrRef::Const(index)), Owned::Str(text)) => {
                program.strings.get(index as usize) == Some(text)
            }
            (Value::Str(StrRef::Arena(handle)), Owned::Str(text)) => {
                matches!(self.get(handle), Ok(Object::Str(there)) if there == text)
            }
            (Value::Obj(ObjRef::Arena(handle)), Owned::Array(items)) => match self.get(handle) {
                Ok(Object::Array(values)) => {
                    values.len() == items.len()
                        && values
                            .iter()
                            .zip(items)
                            .all(|(value, item)| self.matches(*value, item, program))
                }
                _ => false,
            },
            (Value::Obj(ObjRef::Arena(handle)), Owned::Struct { name, fields }) => {
                match self.get(handle) {
                    Ok(Object::Struct {
                        layout,
                        fields: values,
                    }) => {
                        let Some(declared) = program.structs.get(usize::from(*layout)) else {
                            return false;
                        };
                        declared.name == *name
                            && declared.fields.len() == fields.len()
                            && declared.fields.iter().zip(fields).zip(values).all(
                                |(((field, _), (saved, item)), value)| {
                                    field == saved && self.matches(*value, item, program)
                                },
                            )
                    }
                    _ => false,
                }
            }
            (value, Owned::Scalar(scalar)) => value == *scalar,
            _ => false,
        }
    }

    /// How many values `value` holds, nested ones included: `1` for a scalar
    /// or text, `1 +` its parts' sizes for an array or a struct — what copying
    /// it costs.
    pub fn size(&self, value: Value) -> usize {
        match value {
            Value::Obj(ObjRef::Arena(handle)) => match self.get(handle) {
                Ok(Object::Array(items) | Object::Struct { fields: items, .. }) => {
                    1 + items.iter().map(|item| self.size(*item)).sum::<usize>()
                }
                _ => 1,
            },
            _ => 1,
        }
    }

    /// A deep copy of an object, in the arena: the value semantics every
    /// binding of a place has. Text is shared — nothing writes it in place.
    pub fn deep_copy(&mut self, value: Value) -> Result<Value, ArenaError> {
        let Value::Obj(ObjRef::Arena(handle)) = value else {
            return Ok(value);
        };
        let (layout, items) = match self.get(handle)? {
            Object::Array(items) => (None, items.clone()),
            Object::Struct { layout, fields } => (Some(*layout), fields.clone()),
            Object::Str(_) => return Ok(value),
        };
        let copied = items
            .into_iter()
            .map(|item| self.deep_copy(item))
            .collect::<Result<Vec<_>, _>>()?;
        let object = match layout {
            Some(layout) => Object::Struct {
                layout,
                fields: copied,
            },
            None => Object::Array(copied),
        };
        Ok(Value::Obj(ObjRef::Arena(self.alloc(object)?)))
    }
}

/// What a struct field nothing was saved for starts at: its declared default,
/// else its type's zero.
fn filling(layout: &StructLayout, slot: usize, ty: &str) -> Option<Owned> {
    match layout.defaults.get(slot) {
        Some(Some(default)) => Some(default.clone()),
        _ => Owned::zero(ty),
    }
}
