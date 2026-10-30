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

mod applied;
pub use applied::AppliedWrites;
mod apply;
mod json;
mod prefab_spawn;

#[cfg(test)]
mod tests;

pub use apply::ApplyError;

use crate::ecs::SystemContext;
use khora_core::lane::OutputDeck;
use khora_core::script::CommandBuffer;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

/// Applies every queued command, reporting the ones that fail.
///
/// Returns how many were applied, which is what the tests assert on and what a
/// future telemetry counter would read.
pub fn apply_all(world: &mut World, buffer: &CommandBuffer) -> usize {
    apply_all_reporting(world, buffer).applied
}

/// What [`apply_all_reporting`] did with a buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Commands applied — a column write counts once, whatever its rows did.
    pub applied: usize,
    /// Rows of column writes written.
    pub rows_written: usize,
    /// Rows of column writes skipped: an entity gone or without the
    /// component, or a value a field refused.
    pub rows_skipped: usize,
}

/// Applies every queued command as [`apply_all`] does, and reports the rows
/// of column writes too.
pub fn apply_all_reporting(world: &mut World, buffer: &CommandBuffer) -> ApplyReport {
    apply_each(world, buffer, None, |_, _| {})
}

/// Applies every queued command in order, calling `applied` with the index
/// and the command of each that applied.
fn apply_each(
    world: &mut World,
    buffer: &CommandBuffer,
    prefabs: Option<&dyn crate::scene::PrefabSource>,
    mut applied: impl FnMut(usize, &khora_core::script::WorldCommand),
) -> ApplyReport {
    use khora_core::script::WorldCommand;

    let mut report = ApplyReport::default();
    // The spawns refused so far. A turn names the entity it spawns at once, so
    // what it queued after for that id is meant for an entity that never came
    // to be — never for a live one that happens to hold the same id. Column
    // writes are not checked row by row: a spawn the world refuses at its own
    // id is rolled back, so the id names no live entity, and only a reserver
    // the world does not own hands out an id a live entity holds.
    let mut refused: Vec<khora_core::ecs::entity::EntityId> = Vec::new();
    for (index, command) in buffer.iter().enumerate() {
        let names_refused = command.entity().is_some_and(|e| refused.contains(&e))
            || matches!(command, WorldCommand::SetParent { parent: Some(p), .. } if refused.contains(p));
        if names_refused {
            log::error!("script command ignored: it names an entity whose spawn was refused");
            continue;
        }
        match apply::apply_with(world, command, prefabs) {
            Ok(rows) => {
                applied(index, command);
                report.applied += 1;
                report.rows_written += rows.written;
                report.rows_skipped += rows.missing + rows.refused;
            }
            Err(error) => {
                match command {
                    // A column write refused whole skips every row it carried.
                    WorldCommand::WriteColumn { batch, .. } => report.rows_skipped += batch.len(),
                    WorldCommand::Spawn { entity, .. }
                    | WorldCommand::SpawnPrefab { entity, .. } => refused.push(*entity),
                    _ => {}
                }
                log::error!("script command ignored: {error}");
            }
        }
    }
    report
}

fn apply_script_commands(
    world: &mut World,
    runtime: &Runtime,
    deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    // The ids scripts reserved this frame get their slots before anything
    // spawns at them; whatever no spawn used is taken back after — every
    // frame, scripts or not, so no reservation outlives the frame.
    world.settle_reservations();
    let prefabs = runtime
        .resources
        .get::<crate::scene::PrefabLibrary>()
        .map(|library| &*library.0 as &dyn crate::scene::PrefabSource);
    apply_buffer(world, deck, prefabs);
    world.release_unspent();
}

fn apply_buffer(
    world: &mut World,
    deck: &mut OutputDeck,
    prefabs: Option<&dyn crate::scene::PrefabSource>,
) {
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

    // Debug builds only, as `CommandBuffer::warn_on_conflicts`: a behavior's
    // fields are reported by its writeback instead, which knows its turn —
    // what it read and what it wrote — and what applied.
    if cfg!(debug_assertions) {
        for conflict in buffer.conflicts() {
            if let khora_core::script::WriteTarget::Field(component, _) = &conflict.target {
                if is_behavior(world, component.as_str()) {
                    continue;
                }
            }
            log::warn!("{}", conflict.message());
        }
    }
    let mut applied = AppliedWrites::default();
    apply_each(world, &buffer, prefabs, |index, command| {
        applied.record(index, command)
    });
    // For the behaviors' writeback, after this: a turn's field a later
    // command wrote is that command's.
    *deck.slot::<AppliedWrites>() = applied;
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
        observes: &[],
    }
}

/// Whether `component` is a behavior's — what its writeback reports on.
fn is_behavior(world: &World, component: &str) -> bool {
    world
        .components()
        .key_named(component)
        .and_then(|key| world.components().vtable(key))
        .is_some_and(|vtable| {
            vtable
                .script
                .as_ref()
                .and_then(crate::ecs::ScriptRole::runs)
                .is_some()
        })
}
