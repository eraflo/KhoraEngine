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

//! Text that outlives the frame it was built in.
//!
//! Text a program builds lives in the frame arena, which is emptied every
//! frame. A machine that suspends — on an `await`, on a fuel cut — can hold
//! such text in a register and read it frames later, or after a save. So a
//! suspending machine **evacuates**: each arena string its registers and
//! arguments name is copied into the machine, and the register then names that
//! copy ([`StrRef::Held`]). Resuming **rehydrates**: the copies go back into
//! the arena of the frame that resumes, and the registers name them there. The
//! running code never sees a held reference.
//!
//! Neither step is charged to fuel: both happen between instructions, and what
//! they cost depends on how much text the machine holds, not on what it runs.
//!
//! A machine resumed in the frame it stopped in — a budget handed out in
//! slices — finds its text still in the arena: rehydrating reuses it rather
//! than allocating a copy per resume, which would fill the arena with a body
//! that completes when run whole.
//!
//! A field is the other place text outlives a frame, and it is stored by value
//! for the same reason ([`Machine::persist`]).

use std::collections::HashMap;

use super::{Fault, Machine, Program, StrRef, Value};

/// Where each held string was copied from, so a resume in the same arena
/// generation can name it there again.
///
/// A cache, not part of what the machine *is*: never saved, and ignored by
/// equality — a machine read back from a save is the same machine, it only
/// allocates its text once more.
#[derive(Debug, Clone, Default)]
pub(super) struct Origins(Vec<ArenaRef>);

impl PartialEq for Origins {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
use crate::arena::{Arena, ArenaRef, Object, Persisted};
use crate::native::Host;

impl Machine {
    /// Moves the text the machine holds back into `arena`, rewriting each held
    /// reference to the copy there.
    ///
    /// Faults only when the arena is full.
    pub(super) fn rehydrate(&mut self, arena: &mut Arena) -> Result<(), Fault> {
        if self.held.is_empty() {
            return Ok(());
        }
        let origins = std::mem::take(&mut self.origins).0;
        let handles = std::mem::take(&mut self.held)
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                // Still there, holding this very text: the same frame.
                let origin = origins.get(index).copied().filter(
                    |&handle| matches!(arena.get(handle), Ok(Object::Str(there)) if *there == text),
                );
                match origin {
                    Some(handle) => Ok(handle),
                    None => arena.alloc(Object::Str(text)),
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Fault::ArenaFull)?;
        let Self {
            registers,
            arguments,
            ..
        } = self;
        for value in registers.iter_mut().chain(arguments.iter_mut()) {
            if let Value::Str(StrRef::Held(index)) = *value {
                // An index past what the machine holds came from a save nobody
                // checked: it names nothing, and reading it faults.
                let handle = handles
                    .get(index as usize)
                    .copied()
                    .unwrap_or_else(ArenaRef::expired);
                *value = Value::Str(StrRef::Arena(handle));
            }
        }
        Ok(())
    }

    /// Copies every arena string the registers and arguments name into the
    /// machine, rewriting each reference to its held copy.
    ///
    /// A reference already stale is left as it is: it named nothing before the
    /// suspension and still faults when read. Two registers naming the same
    /// text share one copy.
    pub(super) fn evacuate(&mut self, arena: &Arena) {
        let Self {
            registers,
            arguments,
            held,
            origins,
            ..
        } = self;
        let mut copied: HashMap<ArenaRef, u32> = HashMap::new();
        for value in registers.iter_mut().chain(arguments.iter_mut()) {
            let Value::Str(StrRef::Arena(handle)) = *value else {
                continue;
            };
            let index = match copied.get(&handle) {
                Some(&index) => index,
                None => {
                    let Ok(Object::Str(text)) = arena.get(handle) else {
                        continue;
                    };
                    let Ok(index) = u32::try_from(held.len()) else {
                        continue;
                    };
                    held.push(text.clone());
                    origins.0.push(handle);
                    copied.insert(handle, index);
                    index
                }
            };
            *value = Value::Str(StrRef::Held(index));
        }
    }

    /// What a field keeps of `value`: text by value, everything else as it is.
    ///
    /// An arena handle would be stale by the next frame, and a field is exactly
    /// what has to outlive one — a behavior's field and a state's data alike.
    pub(super) fn persist(
        &self,
        value: Value,
        program: &Program,
        host: &Host,
    ) -> Result<Persisted, Fault> {
        Ok(match value {
            Value::Str(_) => {
                let text = self.resolve_str(value, program, host)?.to_owned();
                Persisted::Owned(Object::Str(text))
            }
            other => Persisted::Scalar(other),
        })
    }
}
