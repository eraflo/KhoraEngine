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

//! Where a script's requests become `World` mutations.
//!
//! The other end of [`khora_core::script`]. A script lane cannot write to the
//! `World` — `RULES.md` §3 — so it queues [`WorldCommand`]s into a
//! [`CommandBuffer`] slot on the [`OutputDeck`], and this `Maintenance`-phase
//! `DataSystem` drains the slot and performs them. It is the same
//! Lane → Deck → DataSystem road the audio and physics writebacks take; nothing
//! script-specific reaches the engine loop.
//!
//! # In emission order, always
//!
//! Commands apply exactly in the order they were queued. Grouping them — all the
//! value writes, then all the structural ones — would run faster on paper and be
//! wrong: `AddComponent(Health)` followed by `SetComponent(Health, …)` reads as
//! one thought and would come apart, the write landing before the component
//! existed. Order is the contract.
//!
//! # One bad command is one bad command
//!
//! A script that writes to a despawned entity produces a named diagnostic and
//! the batch continues. Stopping the batch would let one behavior's mistake
//! silently drop every other behavior's work that frame, which is a far harder
//! bug to see than the one that caused it.
//!
//! [`WorldCommand`]: khora_core::script::WorldCommand
//! [`CommandBuffer`]: khora_core::script::CommandBuffer
//! [`OutputDeck`]: khora_core::lane::OutputDeck

mod apply;
mod json;

#[cfg(test)]
mod tests;

pub use apply::ApplyError;

use khora_core::lane::OutputDeck;
use khora_core::script::CommandBuffer;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

/// Applies every queued command, reporting the ones that fail.
///
/// Returns how many were applied, which is what the tests assert on and what a
/// future telemetry counter would read.
pub fn apply_all(world: &mut World, buffer: &CommandBuffer) -> usize {
    let mut applied = 0;
    for command in buffer {
        match apply::apply(world, command) {
            Ok(()) => applied += 1,
            Err(error) => log::error!("script command ignored: {error}"),
        }
    }
    applied
}

fn apply_script_commands(world: &mut World, _runtime: &Runtime, deck: &mut OutputDeck) {
    // A lane that did not run — starved of budget, or absent entirely — leaves
    // no slot. Taking one anyway would fabricate an empty batch and count a
    // frame of work that never happened.
    if !deck.contains::<CommandBuffer>() {
        return;
    }

    let buffer = deck.take::<CommandBuffer>();
    if buffer.is_empty() {
        return;
    }

    buffer.warn_on_conflicts();
    apply_all(world, &buffer);
}

inventory::submit! {
    DataSystemRegistration {
        name: "script_commands",
        phase: TickPhase::Maintenance,
        run: apply_script_commands,
        // After the physics and audio writebacks: those reconcile the World with
        // what the simulation already did, and a script's intent for this frame
        // should read as the last word rather than be overwritten by a
        // simulation result the script was reacting to.
        order_hint: 20,
        runs_after: &[],
    }
}
