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

//! A machine to and from the engine's written-down form.
//!
//! [`FrozenMachine`] is what a scene keeps; [`Machine`] is what runs. The
//! conversion names what the machine numbers — a frame's function, a literal's
//! text — against the program the machine runs in, and resolves those names
//! against the program it is thawed into.
//!
//! Each frame also records the site it stands at, its function's fingerprint,
//! and the locals and temporaries of that site: enough for
//! [`resume`](super::resume) to rebuild it in edited code, after the program
//! that placed them is gone.

use khora_core::script::{FrozenFrame, FrozenLocal, FrozenMachine, FrozenValue, PendingBody};

use super::{Frame, Machine, ObjRef, Program, StrRef, Value};
use crate::arena::{ArenaRef, Owned};

impl Machine {
    /// This machine, written down, owing `body` when it finishes.
    ///
    /// `None` when the machine does not belong to `program` — a frame naming a
    /// function it lacks, a literal outside its table.
    pub fn freeze(&self, program: &Program, body: PendingBody) -> Option<FrozenMachine> {
        let registers = self
            .registers
            .iter()
            .map(|value| freeze_held(*value, program, &self.held))
            .collect::<Option<_>>()?;
        let frames = self
            .frames
            .iter()
            .enumerate()
            .map(|(depth, frame)| {
                let function = program.functions.get(frame.function)?;
                // Where this frame resumes: the machine's counter for the
                // innermost, the return address its callee holds for the rest.
                let pc = match self.frames.get(depth + 1) {
                    Some(callee) => callee.return_pc,
                    None => self.program_counter,
                };
                // The first site at that counter: a function's entry before the
                // statement that starts there, an `await` before the statement
                // that follows it — the place the frame actually stopped. Where
                // a call's return and a checkpoint share a counter, a frame still
                // waiting on its call stands at the return, the innermost one at
                // the checkpoint.
                let innermost = depth + 1 == self.frames.len();
                let at = |site: &&super::Site| site.pc as usize == pc;
                let preferred = |site: &&super::Site| match site.kind {
                    super::SiteKind::Checkpoint => innermost,
                    super::SiteKind::Return { .. } => !innermost,
                    _ => false,
                };
                let site = function
                    .sites
                    .iter()
                    .filter(at)
                    .find(preferred)
                    .or_else(|| function.sites.iter().find(at));
                Some(FrozenFrame {
                    function: function.name.clone(),
                    base: frame.base as u64,
                    return_pc: frame.return_pc as u64,
                    result: frame.result as u64,
                    site: site.map(|site| site.name.clone()).unwrap_or_default(),
                    fingerprint: function.fingerprint,
                    locals: site
                        .map(|site| {
                            site.locals
                                .iter()
                                .map(|local| FrozenLocal {
                                    name: local.name.clone(),
                                    ty: local.ty.clone(),
                                    scope: local.scope.clone(),
                                    register: u64::from(local.register),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    temporaries: site
                        .map(|site| site.temporaries.iter().map(|&reg| u64::from(reg)).collect())
                        .unwrap_or_default(),
                })
            })
            .collect::<Option<_>>()?;

        Some(FrozenMachine {
            body,
            registers,
            frames,
            program_counter: self.program_counter as u64,
            arguments: self
                .arguments
                .iter()
                .map(|value| freeze_held(*value, program, &self.held))
                .collect::<Option<_>>()?,
        })
    }

    /// The machine `frozen` describes, in `program`.
    ///
    /// `None` when `program` cannot hold it: a function or a literal it names
    /// that `program` does not have, or a frame that does not fit its register
    /// file.
    pub fn thaw(frozen: &FrozenMachine, program: &Program) -> Option<Self> {
        let mut held = Vec::new();
        let registers: Vec<Value> = frozen
            .registers
            .iter()
            .map(|value| thaw_held(value, program, &mut held))
            .collect::<Option<_>>()?;
        if frozen.frames.is_empty() {
            return None;
        }

        let frames = frozen
            .frames
            .iter()
            .enumerate()
            .map(|(depth, frame)| thaw_frame(frame, depth, registers.len(), program))
            .collect::<Option<_>>()?;

        let arguments = thaw_arguments(frozen, program, &mut held).unwrap_or_default();
        // A held value the program can no longer hold — a struct renamed, a
        // field retyped or added with neither default nor zero — refuses the
        // thaw rather than faulting when the machine next runs: the resume
        // then rebuilds or restarts the body.
        if !holds_all(&held, program) {
            return None;
        }
        Some(Self {
            registers,
            frames,
            program_counter: usize::try_from(frozen.program_counter).ok()?,
            finished: false,
            arguments,
            held,
            origins: Default::default(),
        })
    }
}

impl Value {
    /// This register, written down.
    ///
    /// `None` for a literal outside `program`'s table.
    pub fn freeze(self, program: &Program) -> Option<FrozenValue> {
        Some(match self {
            Self::Unit => FrozenValue::Unit,
            Self::Int(value) => FrozenValue::Int(value),
            Self::Float(value) => FrozenValue::Float(value),
            Self::Bool(value) => FrozenValue::Bool(value),
            Self::Entity(entity) => FrozenValue::Entity(entity),
            Self::Str(StrRef::Const(index)) => {
                FrozenValue::Literal(program.strings.get(index as usize)?.clone())
            }
            Self::Str(StrRef::Arena(_)) => FrozenValue::Expired,
            // Its text is the machine's, which writes it down itself (see
            // `Machine::freeze`); a held reference alone names nothing.
            Self::Str(StrRef::Held(_)) => FrozenValue::Expired,
            // The same for an array: only its machine can write it down.
            Self::Obj(_) => FrozenValue::Expired,
            Self::Vec2(value) => FrozenValue::Vec2(value),
            Self::Vec3(value) => FrozenValue::Vec3(value),
            Self::Vec4(value) => FrozenValue::Vec4(value),
            Self::Quat(value) => FrozenValue::Quat(value),
            Self::Color(value) => FrozenValue::Color(value),
            Self::Null => FrozenValue::Null,
        })
    }

    /// The register `frozen` describes, in `program`.
    ///
    /// `None` for a literal `program` does not have.
    pub fn thaw(frozen: &FrozenValue, program: &Program) -> Option<Self> {
        Some(match frozen {
            FrozenValue::Unit => Self::Unit,
            FrozenValue::Int(value) => Self::Int(*value),
            FrozenValue::Float(value) => Self::Float(*value),
            FrozenValue::Bool(value) => Self::Bool(*value),
            FrozenValue::Entity(entity) => Self::Entity(*entity),
            FrozenValue::Literal(text) => {
                let index = program.strings.iter().position(|known| known == text)?;
                Self::Str(StrRef::Const(u32::try_from(index).ok()?))
            }
            FrozenValue::Expired => Self::Str(StrRef::Arena(ArenaRef::expired())),
            // Text with no machine to own it: only a machine thaws it (see
            // `Machine::thaw`).
            FrozenValue::Text(_) => return None,
            FrozenValue::Vec2(value) => Self::Vec2(*value),
            FrozenValue::Vec3(value) => Self::Vec3(*value),
            FrozenValue::Vec4(value) => Self::Vec4(*value),
            FrozenValue::Quat(value) => Self::Quat(*value),
            FrozenValue::Color(value) => Self::Color(*value),
            FrozenValue::Null => Self::Null,
            // Like text, an array or a struct is owned by the machine that
            // thaws it.
            FrozenValue::Array(_) | FrozenValue::Struct { .. } => return None,
        })
    }
}

/// Whether every held value can come back into `program`'s arena.
pub(super) fn holds_all(held: &[Owned], program: &Program) -> bool {
    let mut scratch = crate::arena::Arena::new();
    held.iter()
        .all(|owned| scratch.import(owned, program).is_ok())
}

/// The body's arguments, in `program`, their text added to `held`.
///
/// `None` for one `program` cannot hold — a literal it lacks. Only a restart
/// reads them, so the machine itself thaws without them.
pub(super) fn thaw_arguments(
    frozen: &FrozenMachine,
    program: &Program,
    held: &mut Vec<Owned>,
) -> Option<Vec<Value>> {
    frozen
        .arguments
        .iter()
        .map(|value| thaw_held(value, program, held))
        .collect()
}

/// A register of a machine holding `held`, written down: held text and arrays
/// by value.
pub(super) fn freeze_held(value: Value, program: &Program, held: &[Owned]) -> Option<FrozenValue> {
    match value {
        Value::Str(StrRef::Held(index)) | Value::Obj(ObjRef::Held(index)) => {
            match held.get(index as usize) {
                Some(owned) => freeze_owned(owned, program),
                None => Some(FrozenValue::Expired),
            }
        }
        other => other.freeze(program),
    }
}

/// An owned value, written down.
fn freeze_owned(owned: &Owned, program: &Program) -> Option<FrozenValue> {
    Some(match owned {
        Owned::Scalar(value) => value.freeze(program)?,
        Owned::Str(text) => FrozenValue::Text(text.clone()),
        Owned::Array(items) => FrozenValue::Array(
            items
                .iter()
                .map(|item| freeze_owned(item, program))
                .collect::<Option<_>>()?,
        ),
        Owned::Struct { name, fields } => FrozenValue::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(field, value)| Some((field.clone(), freeze_owned(value, program)?)))
                .collect::<Option<_>>()?,
        },
    })
}

/// A register, in `program`, for a machine holding `held`: text and arrays it
/// owned go back into `held`, and the register names them there.
pub(super) fn thaw_held(
    frozen: &FrozenValue,
    program: &Program,
    held: &mut Vec<Owned>,
) -> Option<Value> {
    match frozen {
        FrozenValue::Text(_) | FrozenValue::Array(_) | FrozenValue::Struct { .. } => {
            let index = u32::try_from(held.len()).ok()?;
            held.push(thaw_owned(frozen, program)?);
            Some(match frozen {
                FrozenValue::Text(_) => Value::Str(StrRef::Held(index)),
                _ => Value::Obj(ObjRef::Held(index)),
            })
        }
        other => Value::thaw(other, program),
    }
}

/// The owned value a written-down one describes. `None` for one no machine
/// could have held — text that had expired, a literal `program` lacks.
fn thaw_owned(frozen: &FrozenValue, program: &Program) -> Option<Owned> {
    Some(match frozen {
        FrozenValue::Text(text) | FrozenValue::Literal(text) => Owned::Str(text.clone()),
        FrozenValue::Array(items) => Owned::Array(
            items
                .iter()
                .map(|item| thaw_owned(item, program))
                .collect::<Option<_>>()?,
        ),
        FrozenValue::Struct { name, fields } => Owned::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(field, value)| Some((field.clone(), thaw_owned(value, program)?)))
                .collect::<Option<_>>()?,
        },
        FrozenValue::Expired => return None,
        scalar => Owned::Scalar(Value::thaw(scalar, program)?),
    })
}

/// One frame, if `program` has its function and the register file holds it.
///
/// The VM trusts a frame's window to lie inside the register file — the
/// compiler sized it so. A frame read from a save was sized by nobody, so the
/// window is checked here, once, rather than on every register access.
fn thaw_frame(
    frame: &FrozenFrame,
    depth: usize,
    registers: usize,
    program: &Program,
) -> Option<Frame> {
    let function = program.index_of(&frame.function)?;
    let base = usize::try_from(frame.base).ok()?;
    let end = base.checked_add(program.functions.get(function)?.registers)?;
    let result = usize::try_from(frame.result).ok()?;
    // The outermost frame's result is never read — its value goes to register
    // zero — so it is held to what a fresh machine writes there.
    let result_fits = if depth == 0 {
        result == 0
    } else {
        result < registers
    };
    if end > registers || !result_fits {
        return None;
    }

    Some(Frame {
        function,
        base,
        return_pc: usize::try_from(frame.return_pc).ok()?,
        result,
    })
}
