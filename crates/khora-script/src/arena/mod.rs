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

//! Frame-scoped memory.
//!
//! Ergon has no garbage collector, which is the point: a collector is one more
//! consumer for the DCC to budget, and an unpredictable pause is exactly what a
//! frame budget cannot absorb. Instead, memory is split by lifetime and each
//! kind is freed by knowing *when*, not by discovering *whether*.
//!
//! | Zone | Freed | Holds |
//! |---|---|---|
//! | This arena | every frame, all at once | temporaries, arrays, strings |
//! | [`persistent`] | with the entity | behavior fields, state, continuations |
//! | The ECS | with the scene | what the game authored |
//!
//! Releasing a frame's allocations costs one counter bump, whatever was
//! allocated. Nothing is traced and nothing is scanned.
//!
//! # The danger, and the guard
//!
//! Reusing memory wholesale means a reference held past the reset would read
//! whatever landed there next — a use-after-free with none of the noise.
//!
//! So every reference carries the **generation** it was made in, and `reset`
//! bumps it. A stale reference does not read the wrong object: it does not
//! read at all. [`Arena::get`] returns [`ArenaError::Stale`], which says how
//! many frames ago the value died and where to keep one that has to live
//! longer. The check is a `u32` comparison on each access — cheap next to what
//! it rules out, and unlike a collector its cost does not vary from frame to
//! frame.

pub mod persistent;

#[cfg(test)]
mod tests;

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
}

/// Something too large to live in a register.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Object {
    /// `T[]`
    Array(Vec<Value>),
    /// A string.
    Str(String),
}

impl Object {
    /// The element count of an array, or the character count of a string.
    pub fn len(&self) -> usize {
        match self {
            Self::Array(values) => values.len(),
            // Characters, not bytes: `Length` should agree with what the author
            // can count, which is what the diagnostics already assume.
            Self::Str(text) => text.chars().count(),
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
    pub fn get(&self, reference: ArenaRef) -> Result<&Object, ArenaError> {
        self.check(reference)?;
        self.objects
            .get(reference.index as usize)
            .ok_or(ArenaError::OutOfBounds)
    }

    /// Reads through a reference, mutably.
    pub fn get_mut(&mut self, reference: ArenaRef) -> Result<&mut Object, ArenaError> {
        self.check(reference)?;
        self.objects
            .get_mut(reference.index as usize)
            .ok_or(ArenaError::OutOfBounds)
    }

    /// Frees everything at once.
    ///
    /// The bump: one counter, whatever was allocated. Every outstanding
    /// reference becomes detectably stale in the same step — which is what
    /// makes freeing wholesale safe rather than reckless.
    pub fn reset(&mut self) {
        self.objects.clear();
        // Wrapping keeps the invariant at the boundary: after u32::MAX frames
        // the counter returns to 1 rather than to 0, so it never collides with
        // the "never allocated" value. At 60fps that is over two years of
        // continuous play, but a wrong answer then would be no better for
        // being rare.
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
