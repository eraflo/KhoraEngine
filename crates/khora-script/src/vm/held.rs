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

//! Text and arrays that outlive the frame they were built in.
//!
//! What a program builds — text, an array — lives in the frame arena, which is
//! emptied every frame. A machine that suspends — on an `await`, on a fuel cut —
//! can hold such a value in a register and read it frames later, or after a
//! save. So a suspending machine **evacuates**: each arena value its registers
//! and arguments name is copied out, whole ([`Owned`]), into the machine, and
//! the register then names that copy ([`StrRef::Held`], [`ObjRef::Held`]).
//! Resuming **rehydrates**: the copies go back into the arena of the frame that
//! resumes, and the registers name them there. The running code never sees a
//! held reference.
//!
//! Neither step is charged to fuel: both happen between instructions, and what
//! they cost depends on how much the machine holds, not on what it runs.
//!
//! A machine resumed in the frame it stopped in — a budget handed out in
//! slices — finds what it held still in the arena: rehydrating reuses it
//! rather than allocating a copy per resume, which would fill the arena with a
//! body that completes when run whole.
//!
//! A field is the other place a value outlives a frame, and it is stored by
//! value for the same reason ([`Machine::persist`]).

use std::collections::HashMap;

use super::{Fault, Machine, ObjRef, Program, StrRef, Value};
use crate::arena::{Arena, ArenaRef, Owned, Persisted};
use crate::native::Host;

/// Where each held value was copied from, so a resume in the same arena
/// generation can name it there again.
///
/// A cache, not part of what the machine *is*: never saved, and ignored by
/// equality — a machine read back from a save is the same machine, it only
/// allocates what it holds once more.
#[derive(Debug, Clone, Default)]
pub(super) struct Origins(Vec<Value>);

impl PartialEq for Origins {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The arena handle a text or object value names, if it names one.
fn arena_handle(value: Value) -> Option<ArenaRef> {
    match value {
        Value::Str(StrRef::Arena(handle)) | Value::Obj(ObjRef::Arena(handle)) => Some(handle),
        _ => None,
    }
}

/// The held reference of the same kind as `value`, at `index`.
fn held_like(value: Value, index: u32) -> Value {
    match value {
        Value::Obj(_) => Value::Obj(ObjRef::Held(index)),
        _ => Value::Str(StrRef::Held(index)),
    }
}

impl Machine {
    /// Moves what the machine holds back into `arena`, rewriting each held
    /// reference to the copy there.
    ///
    /// Faults only when the arena is full.
    pub(super) fn rehydrate(&mut self, arena: &mut Arena, program: &Program) -> Result<(), Fault> {
        if self.held.is_empty() {
            return Ok(());
        }
        let origins = std::mem::take(&mut self.origins).0;
        let values = std::mem::take(&mut self.held)
            .iter()
            .enumerate()
            .map(|(index, owned)| {
                // Still there, holding this very value: the same frame.
                let origin = origins
                    .get(index)
                    .copied()
                    .filter(|&value| arena.matches(value, owned, program));
                match origin {
                    Some(value) => Ok(value),
                    None => arena.import(owned, program),
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| match error {
                crate::arena::ArenaError::Full => Fault::ArenaFull,
                _ => Fault::NotAnObject {
                    found: "a held value the script no longer declares",
                },
            })?;
        let Self {
            registers,
            arguments,
            ..
        } = self;
        for value in registers.iter_mut().chain(arguments.iter_mut()) {
            let index = match *value {
                Value::Str(StrRef::Held(index)) | Value::Obj(ObjRef::Held(index)) => index,
                _ => continue,
            };
            // An index past what the machine holds came from a save nobody
            // checked: it names nothing, and reading it faults.
            *value = values
                .get(index as usize)
                .copied()
                .unwrap_or_else(|| match *value {
                    Value::Obj(_) => Value::Obj(ObjRef::Arena(ArenaRef::expired())),
                    _ => Value::Str(StrRef::Arena(ArenaRef::expired())),
                });
        }
        Ok(())
    }

    /// Copies every arena value the registers and arguments name into the
    /// machine, rewriting each reference to its held copy.
    ///
    /// A reference already stale is left as it is: it named nothing before the
    /// suspension and still faults when read. Two registers naming the same
    /// value share one copy.
    pub(super) fn evacuate(&mut self, arena: &Arena, program: &Program) {
        let Self {
            registers,
            arguments,
            held,
            origins,
            ..
        } = self;
        let mut copied: HashMap<ArenaRef, u32> = HashMap::new();
        for value in registers.iter_mut().chain(arguments.iter_mut()) {
            let Some(handle) = arena_handle(*value) else {
                continue;
            };
            let index = match copied.get(&handle) {
                Some(&index) => index,
                None => {
                    let Ok(owned) = arena.export(*value, program) else {
                        continue;
                    };
                    let Ok(index) = u32::try_from(held.len()) else {
                        continue;
                    };
                    held.push(owned);
                    origins.0.push(*value);
                    copied.insert(handle, index);
                    index
                }
            };
            *value = held_like(*value, index);
        }
    }

    /// What a field keeps of `value`: text and arrays by value, everything else
    /// as it is.
    ///
    /// An arena handle would be stale by the next frame, and a field is exactly
    /// what has to outlive one — a behavior's field and a state's data alike.
    pub(super) fn persist(
        &self,
        value: Value,
        program: &Program,
        host: &Host,
    ) -> Result<Persisted, Fault> {
        if !value.is_reference() {
            return Ok(Persisted::Scalar(value));
        }
        host.arena
            .export(value, program)
            .map(|owned| match owned {
                Owned::Scalar(scalar) => Persisted::Scalar(scalar),
                owned => Persisted::Owned(owned),
            })
            .map_err(|_| match value {
                Value::Obj(_) => Fault::NotAnObject {
                    found: "an array that no longer exists",
                },
                _ => Fault::BadString,
            })
    }
}
