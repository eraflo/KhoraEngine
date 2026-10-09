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

//! Arrays at run time: building, indexing, measuring, copying — and what each
//! costs.
//!
//! An array is an arena object a register names by handle. Ergon gives it
//! value semantics: binding one read from a place copies it (`Copy`, emitted by
//! the compiler), so two names never share one, and writing through a path
//! mutates in place.
//!
//! # Sized operations
//!
//! Most instructions cost one unit of fuel. A few do work proportional to a
//! size only the running program knows — a deep copy, a field read or write
//! that copies an array across the frame boundary — and are charged that
//! size. Fuel is otherwise checked only at safepoints, so a sized operation is
//! its own checkpoint: it does not start when the remaining fuel cannot pay
//! for it, unless it is the first thing the run does (every run must make
//! progress). The compiler names a site at each one, so a frame stopped there
//! resumes, and rebuilds, like any other.

use super::{Fault, Instruction, Machine, ObjRef, Program, Step, Value};
use crate::arena::{Arena, Object, Persisted};
use crate::native::Host;

impl Machine {
    /// What `instruction` costs when that depends on the values it touches —
    /// or `None` when its cost is fixed. A run stops before one it cannot pay
    /// for.
    ///
    /// A write that completes an update, a state's entry or a `become`
    /// (`WriteBack`, `Become`) is not among them: it is never a place to stop,
    /// and the value it writes was already paid for, as it was loaded, copied
    /// or built in the same run. Charging it again would let a run overdraw by
    /// a size no static bound knows.
    pub(super) fn sized_cost(&self, instruction: &Instruction, host: &Host) -> Option<u64> {
        let size = match *instruction {
            Instruction::Copy { src, .. } => host.arena.size(self.read(src).ok()?),
            Instruction::LoadField { slot, .. } => match host.fields.get(slot as usize)? {
                Persisted::Owned(owned) if owned.is_object() => owned.size(),
                _ => return None,
            },
            Instruction::RemoveAt {
                array: target,
                index,
                charged: true,
                ..
            } => {
                let len = array(&host.arena, self.read(target).ok()?).ok()?.len();
                let at = self.read_index(index).ok()?;
                let shifted = usize::try_from(at)
                    .ok()
                    .filter(|&at| at < len)
                    .map_or(0, |at| len - at - 1);
                return Some(1 + shifted as u64);
            }
            Instruction::StoreField { src, .. } => {
                let value = self.read(src).ok()?;
                value.as_obj()?;
                host.arena.size(value)
            }
            _ => return None,
        };
        Some(1 + size as u64)
    }

    /// Executes an array instruction.
    pub(super) fn step_object(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        match *instruction {
            Instruction::NewArray { dst, base, count } => {
                let values = self.read_run(base, count)?;
                let handle = host
                    .arena
                    .alloc(Object::Array(values))
                    .map_err(|_| Fault::ArenaFull)?;
                self.write(dst, Value::Obj(ObjRef::Arena(handle)))?;
            }
            Instruction::Extend { array, base, count } => {
                let values = self.read_run(base, count)?;
                let target = self.read(array)?;
                array_mut(&mut host.arena, target)?.extend(values);
            }
            Instruction::GetIndex { dst, object, index } => {
                let target = self.read(object)?;
                let at = self.read_index(index)?;
                let items = array(&host.arena, target)?;
                let value = *items
                    .get(position(at, items.len())?)
                    .unwrap_or(&Value::Unit);
                self.write(dst, value)?;
            }
            Instruction::SetIndex { object, index, src } => {
                let target = self.read(object)?;
                let at = self.read_index(index)?;
                let value = self.read(src)?;
                let items = array_mut(&mut host.arena, target)?;
                let slot = position(at, items.len())?;
                items[slot] = value;
            }
            Instruction::Length { dst, src } => {
                let value = self.read(src)?;
                let length = match value {
                    Value::Str(_) => self.resolve_str(value, program, host)?.chars().count(),
                    other => array(&host.arena, other)?.len(),
                };
                self.write(dst, Value::Int(length as i64))?;
            }
            Instruction::NewStruct {
                dst,
                layout,
                base,
                count,
            } => {
                let fields = self.read_run(base, count)?;
                let handle = host
                    .arena
                    .alloc(Object::Struct { layout, fields })
                    .map_err(|_| Fault::ArenaFull)?;
                self.write(dst, Value::Obj(ObjRef::Arena(handle)))?;
            }
            Instruction::GetField {
                dst, object, slot, ..
            } => {
                let target = self.read(object)?;
                let value = *fields(&host.arena, target)?.get(usize::from(slot)).ok_or(
                    Fault::NotAnObject {
                        found: "a struct without that field",
                    },
                )?;
                self.write(dst, value)?;
            }
            Instruction::SetField {
                object, slot, src, ..
            } => {
                let target = self.read(object)?;
                let value = self.read(src)?;
                let field = fields_mut(&mut host.arena, target)?
                    .get_mut(usize::from(slot))
                    .ok_or(Fault::NotAnObject {
                        found: "a struct without that field",
                    })?;
                *field = value;
            }
            Instruction::Push { array: target, src } => {
                let target = self.read(target)?;
                let value = self.read(src)?;
                array_mut(&mut host.arena, target)?.push(value);
            }
            Instruction::RemoveAt {
                array: target,
                index,
                ..
            } => {
                let target = self.read(target)?;
                let at = self.read_index(index)?;
                let items = array_mut(&mut host.arena, target)?;
                let slot = position(at, items.len())?;
                items.remove(slot);
            }
            Instruction::Copy { dst, src } => {
                let value = self.read(src)?;
                let copy = host.arena.deep_copy(value).map_err(|error| match error {
                    crate::arena::ArenaError::Full => Fault::ArenaFull,
                    _ => Fault::NotAnObject {
                        found: "an array that no longer exists",
                    },
                })?;
                self.write(dst, copy)?;
            }
            _ => unreachable!("only array instructions are routed here"),
        }
        Ok(Step::Next)
    }

    /// `count` consecutive registers from `base`.
    fn read_run(&self, base: super::Reg, count: u16) -> Result<Vec<Value>, Fault> {
        (0..count)
            .map(|offset| {
                let register = usize::from(base) + usize::from(offset);
                let register = super::Reg::try_from(register).map_err(|_| Fault::BadRegister {
                    index: super::Reg::MAX,
                })?;
                self.read(register)
            })
            .collect()
    }

    fn read_index(&self, index: super::Reg) -> Result<i64, Fault> {
        let value = self.read(index)?;
        value.as_int().ok_or(Fault::TypeMismatch {
            expected: "int",
            found: value.type_name(),
        })
    }
}

/// The elements of the array `value` names.
fn array(arena: &Arena, value: Value) -> Result<&Vec<Value>, Fault> {
    match value {
        Value::Obj(ObjRef::Arena(handle)) => match arena.get(handle) {
            Ok(Object::Array(items)) => Ok(items),
            _ => Err(Fault::NotAnObject {
                found: "an array that no longer exists",
            }),
        },
        other => Err(Fault::NotAnObject {
            found: other.type_name(),
        }),
    }
}

/// The elements of the array `value` names, to write.
fn array_mut(arena: &mut Arena, value: Value) -> Result<&mut Vec<Value>, Fault> {
    match value {
        Value::Obj(ObjRef::Arena(handle)) => match arena.get_mut(handle) {
            Ok(Object::Array(items)) => Ok(items),
            _ => Err(Fault::NotAnObject {
                found: "an array that no longer exists",
            }),
        },
        other => Err(Fault::NotAnObject {
            found: other.type_name(),
        }),
    }
}

/// The fields of the struct `value` names.
fn fields(arena: &Arena, value: Value) -> Result<&Vec<Value>, Fault> {
    match value {
        Value::Obj(ObjRef::Arena(handle)) => match arena.get(handle) {
            Ok(Object::Struct { fields, .. }) => Ok(fields),
            _ => Err(Fault::NotAnObject {
                found: "a struct that no longer exists",
            }),
        },
        other => Err(Fault::NotAnObject {
            found: other.type_name(),
        }),
    }
}

/// The fields of the struct `value` names, to write.
fn fields_mut(arena: &mut Arena, value: Value) -> Result<&mut Vec<Value>, Fault> {
    match value {
        Value::Obj(ObjRef::Arena(handle)) => match arena.get_mut(handle) {
            Ok(Object::Struct { fields, .. }) => Ok(fields),
            _ => Err(Fault::NotAnObject {
                found: "a struct that no longer exists",
            }),
        },
        other => Err(Fault::NotAnObject {
            found: other.type_name(),
        }),
    }
}

/// `index` as a position in an array of `len`, or the fault naming both.
fn position(index: i64, len: usize) -> Result<usize, Fault> {
    usize::try_from(index)
        .ok()
        .filter(|&at| at < len)
        .ok_or(Fault::IndexOutOfRange { index, len })
}
