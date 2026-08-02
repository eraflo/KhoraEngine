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

//! Where commands wait for the boundary.
//!
//! The buffer is the [`OutputDeck`] slot a script lane writes and the engine
//! drains. Two properties earn it a type of its own rather than a bare `Vec`.
//!
//! # Order is a guarantee, not an accident
//!
//! Commands apply in the order they were emitted. If the lane runs behaviors in
//! parallel, "the order they were emitted" stops being well defined — worker
//! completion order is not reproducible, and a replay or a networked client
//! would diverge from the same inputs. [`CommandBuffer::merge_ordered`] therefore
//! merges per-worker buffers by an explicit key rather than by whoever finished
//! first.
//!
//! # Lost writes are reported, not discovered
//!
//! Two scripts placing the same entity is last-one-wins, which is a defensible
//! rule and an awful thing to debug in silence. [`CommandBuffer::conflicts`]
//! names them. What it deliberately does *not* flag is two scripts *nudging* the
//! same entity: those compose, nothing is lost, and warning about it would train
//! the author to ignore the warning.
//!
//! [`OutputDeck`]: crate::lane::deck::OutputDeck

use std::collections::HashMap;

use crate::ecs::entity::EntityId;

use super::{WorldCommand, WriteTarget};

/// Two or more absolute writes landing on the same thing in one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The entity written more than once.
    pub entity: EntityId,
    /// What was written.
    pub target: WriteTarget,
    /// How many writes landed on it.
    pub writes: usize,
}

impl Conflict {
    /// A message naming what was lost and how the tie was broken.
    pub fn message(&self) -> String {
        format!(
            "entity {}v{}: {} written {} times this frame — the last write wins, \
             the others are lost",
            self.entity.index, self.entity.generation, self.target, self.writes
        )
    }
}

/// Commands awaiting the frame boundary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandBuffer {
    commands: Vec<WorldCommand>,
}

impl CommandBuffer {
    /// An empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty buffer with room for `capacity` commands.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// Queues a command.
    pub fn push(&mut self, command: WorldCommand) {
        self.commands.push(command);
    }

    /// How many commands are queued.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The queued commands, in emission order.
    pub fn as_slice(&self) -> &[WorldCommand] {
        &self.commands
    }

    /// Iterates the queued commands, in emission order.
    pub fn iter(&self) -> std::slice::Iter<'_, WorldCommand> {
        self.commands.iter()
    }

    /// Takes every command, leaving the buffer empty and its allocation intact.
    ///
    /// Keeping the allocation is the point of draining rather than replacing:
    /// the buffer is refilled every frame, and a frame that had to grow it once
    /// should not have to grow it again.
    pub fn drain(&mut self) -> std::vec::Drain<'_, WorldCommand> {
        self.commands.drain(..)
    }

    /// Discards every command.
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Moves `other`'s commands onto the end of this buffer, emptying it.
    pub fn append(&mut self, other: &mut Self) {
        self.commands.append(&mut other.commands);
    }

    /// Merges buffers by an explicit order key.
    ///
    /// The key is what makes a parallel lane reproducible: sorting by it means
    /// the applied order depends on the scene, not on which worker finished
    /// first. Behavior declaration order is the intended key.
    pub fn merge_ordered(parts: impl IntoIterator<Item = (u64, CommandBuffer)>) -> Self {
        let mut parts: Vec<_> = parts.into_iter().collect();
        // Stable, so two parts sharing a key keep the order they were handed in
        // rather than being permuted by the sort itself.
        parts.sort_by_key(|(key, _)| *key);

        let total = parts.iter().map(|(_, buffer)| buffer.len()).sum();
        let mut merged = Self::with_capacity(total);
        for (_, mut buffer) in parts {
            merged.append(&mut buffer);
        }
        merged
    }

    /// Finds the writes that will be silently lost when this buffer is applied.
    ///
    /// Returned in a fixed order so a test — or a log a user is comparing across
    /// two runs — does not change shape between runs of the same frame.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut counts: HashMap<(EntityId, WriteTarget), usize> = HashMap::new();
        for command in &self.commands {
            if let Some(key) = command.write_target() {
                *counts.entry(key).or_insert(0) += 1;
            }
        }

        let mut conflicts: Vec<_> = counts
            .into_iter()
            .filter(|(_, writes)| *writes > 1)
            .map(|((entity, target), writes)| Conflict {
                entity,
                target,
                writes,
            })
            .collect();
        conflicts.sort_by(|a, b| {
            (a.entity.index, a.entity.generation, &a.target).cmp(&(
                b.entity.index,
                b.entity.generation,
                &b.target,
            ))
        });
        conflicts
    }

    /// Reports lost writes to the log, in debug builds only.
    ///
    /// The guard lives here rather than at the call site so the cost — a hash
    /// per command — cannot be left switched on in a shipped build by someone
    /// who did not know to wrap the call.
    pub fn warn_on_conflicts(&self) {
        if !cfg!(debug_assertions) {
            return;
        }
        for conflict in self.conflicts() {
            log::warn!("{}", conflict.message());
        }
    }
}

impl Extend<WorldCommand> for CommandBuffer {
    fn extend<T: IntoIterator<Item = WorldCommand>>(&mut self, iter: T) {
        self.commands.extend(iter);
    }
}

impl<'a> IntoIterator for &'a CommandBuffer {
    type Item = &'a WorldCommand;
    type IntoIter = std::slice::Iter<'a, WorldCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for CommandBuffer {
    type Item = WorldCommand;
    type IntoIter = std::vec::IntoIter<WorldCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.into_iter()
    }
}
