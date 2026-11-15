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

//! Frame-scoped memory: Ergon has no garbage collector, so memory is freed by lifetime.
//!
//! | Zone | Freed | Holds |
//! |---|---|---|
//! | This arena | every frame, all at once | temporaries, arrays, strings |
//! | [`persistent`] | with the entity | behavior fields, state, continuations |
//! | The ECS | with the scene | what the game authored |
//!
//! Every reference carries the **generation** it was made in, and a reset
//! bumps it: a reference held past its frame reads nothing, [`Arena::get`]
//! returns [`ArenaError::Stale`].

pub mod owned;
pub mod persistent;

pub use owned::Owned;
pub use persistent::{Persisted, PersistentStore};

use serde::{Deserialize, Serialize};

use crate::vm::Value;

/// A handle to something in the arena.
///
/// Carries the generation it was minted in so a reference that outlived its
/// frame can be told apart from a live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArenaRef {
    index: u32,
    generation: u32,
}

impl ArenaRef {
    /// The generation this reference belongs to.
    pub fn generation(self) -> u32 {
        self.generation
    }

    /// A reference to nothing, in no frame: every arena refuses it.
    ///
    /// What text built while running becomes once its frame is gone for good —
    /// after a save, say. Its generation is one no arena reaches in practice
    /// and its index one no arena can hold, so either check turns it away.
    pub(crate) fn expired() -> Self {
        Self {
            index: u32::MAX,
            generation: u32::MAX,
        }
    }
}

/// Something too large to live in a register.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Object {
    /// `T[]`
    Array(Vec<Value>),
    /// A string.
    Str(String),
    /// A struct: its fields in declaration order. `layout` indexes
    /// [`Program::structs`](crate::vm::Program::structs).
    Struct {
        /// Which struct.
        layout: u16,
        /// Its fields' values, in declaration order.
        fields: Vec<Value>,
    },
}

impl Object {
    /// The element count of an array, or the character count of a string.
    pub fn len(&self) -> usize {
        match self {
            Self::Array(values) => values.len(),
            // Characters, not bytes: `Length` should agree with what the author
            // can count, which is what the diagnostics already assume.
            Self::Str(text) => text.chars().count(),
            Self::Struct { fields, .. } => fields.len(),
        }
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Short type name, for fault messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Array(_) => "array",
            Self::Str(_) => "string",
            Self::Struct { .. } => "struct",
        }
    }
}

/// Why an arena access failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// The reference was made in an earlier frame.
    ///
    /// Not a technicality: it means a value was kept somewhere it cannot
    /// outlive, which is the one mistake this design can make.
    Stale {
        /// The generation the reference carried.
        reference: u32,
        /// The arena's generation now.
        current: u32,
    },
    /// The index is outside the arena.
    OutOfBounds,
    /// The arena is full.
    Full,
    /// A value the program's declarations cannot hold — a struct it does not
    /// declare, or one missing a field that has neither a default nor a zero.
    Mismatch,
}

impl ArenaError {
    /// A message naming what went wrong.
    pub fn message(self) -> String {
        match self {
            Self::Stale { reference, current } => format!(
                "this value was allocated {} frame(s) ago and no longer exists",
                current.saturating_sub(reference)
            ),
            Self::OutOfBounds => "this value is not in the arena".to_owned(),
            Self::Full => "the frame arena is full".to_owned(),
            Self::Mismatch => "this value does not fit what the script declares".to_owned(),
        }
    }

    /// The reasoning, so the rule reads as a design rather than a limit.
    pub fn note(self) -> &'static str {
        match self {
            Self::Stale { .. } => {
                "arrays and strings live for one frame; to keep one, put it in a behavior field or in the ECS"
            }
            Self::OutOfBounds => "the reference does not point into this arena",
            Self::Full => "a single frame allocated more than the arena holds — a loop building an array without bound is the usual cause",
            Self::Mismatch => "a saved value is matched to the script by field name; a field with neither a default nor a zero must be saved",
        }
    }
}

/// How many objects one frame may allocate.
///
/// A bound rather than unbounded growth: a script looping on `[…]` would
/// otherwise take the process down. Hitting the ceiling faults one behavior;
/// growing without limit takes the whole game.
const CAPACITY: usize = 65_536;

/// A bump allocator emptied once per frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arena {
    objects: Vec<Object>,
    generation: u32,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    /// An empty arena.
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            // Starting at 1 leaves 0 free to mean "never allocated", so a
            // default-constructed reference cannot alias a live object.
            generation: 1,
        }
    }

    /// The current generation.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// How many objects are live.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether nothing is allocated.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Allocates `object`, returning a reference valid until the next reset.
    ///
    /// # Errors
    ///
    /// [`ArenaError::Full`] past the frame's capacity.
    pub fn alloc(&mut self, object: Object) -> Result<ArenaRef, ArenaError> {
        if self.objects.len() >= CAPACITY {
            return Err(ArenaError::Full);
        }
        let index = self.objects.len() as u32;
        self.objects.push(object);
        Ok(ArenaRef {
            index,
            generation: self.generation,
        })
    }

    /// Reads through a reference.
    ///
    /// # Errors
    ///
    /// [`ArenaError::Stale`] for a reference of another frame,
    /// [`ArenaError::OutOfBounds`] for one past the arena.
    pub fn get(&self, reference: ArenaRef) -> Result<&Object, ArenaError> {
        self.check(reference)?;
        self.objects
            .get(reference.index as usize)
            .ok_or(ArenaError::OutOfBounds)
    }

    /// Reads through a reference, mutably.
    ///
    /// # Errors
    ///
    /// As [`Arena::get`].
    pub fn get_mut(&mut self, reference: ArenaRef) -> Result<&mut Object, ArenaError> {
        self.check(reference)?;
        self.objects
            .get_mut(reference.index as usize)
            .ok_or(ArenaError::OutOfBounds)
    }

    /// Frees everything at once; every outstanding reference becomes stale.
    pub fn reset(&mut self) {
        self.objects.clear();
        // After u32::MAX frames the counter wraps to 1, never to 0: it never
        // collides with the "never allocated" value, however rare that frame.
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    /// Whether a reference belongs to this generation.
    fn check(&self, reference: ArenaRef) -> Result<(), ArenaError> {
        if reference.generation != self.generation {
            return Err(ArenaError::Stale {
                reference: reference.generation,
                current: self.generation,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
