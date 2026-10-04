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

use super::{Frame, Machine, Program, StrRef, Value};
use crate::arena::ArenaRef;

impl Machine {
    /// This machine, written down, owing `body` when it finishes.
    ///
    /// `None` when the machine does not belong to `program` — a frame naming a
    /// function it lacks, a literal outside its table.
    pub fn freeze(&self, program: &Program, body: PendingBody) -> Option<FrozenMachine> {
        let registers = self
            .registers
            .iter()
            .map(|value| value.freeze(program))
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
                // that follows it — the place the frame actually stopped.
                let site = function.sites.iter().find(|site| site.pc as usize == pc);
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
                .map(|value| value.freeze(program))
                .collect::<Option<_>>()?,
        })
    }

    /// The machine `frozen` describes, in `program`.
    ///
    /// `None` when `program` cannot hold it: a function or a literal it names
    /// that `program` does not have, or a frame that does not fit its register
    /// file.
    pub fn thaw(frozen: &FrozenMachine, program: &Program) -> Option<Self> {
        let registers: Vec<Value> = frozen
            .registers
            .iter()
            .map(|value| Value::thaw(value, program))
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

        Some(Self {
            registers,
            frames,
            program_counter: usize::try_from(frozen.program_counter).ok()?,
            finished: false,
            arguments: thaw_arguments(frozen, program).unwrap_or_default(),
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
            FrozenValue::Vec2(value) => Self::Vec2(*value),
            FrozenValue::Vec3(value) => Self::Vec3(*value),
            FrozenValue::Vec4(value) => Self::Vec4(*value),
            FrozenValue::Quat(value) => Self::Quat(*value),
            FrozenValue::Color(value) => Self::Color(*value),
            FrozenValue::Null => Self::Null,
        })
    }
}

/// The body's arguments, in `program`.
///
/// `None` for one `program` cannot hold — a literal it lacks. Only a restart
/// reads them, so the machine itself thaws without them.
pub(super) fn thaw_arguments(frozen: &FrozenMachine, program: &Program) -> Option<Vec<Value>> {
    frozen
        .arguments
        .iter()
        .map(|value| Value::thaw(value, program))
        .collect()
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
