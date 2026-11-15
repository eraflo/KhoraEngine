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

//! Applies the [`WorldCommand`]s scripts queued in the deck's [`CommandBuffer`]
//! to the World.
//!
//! Commands apply in the order they were queued: an `AddComponent` then a
//! `SetComponent` of it stay one thought. A failing command is reported and the
//! batch goes on, so one behavior's mistake never drops the others' work.
//! See the book: concepts/scripting/writing.md, "A script never writes the world".
//!
//! [`WorldCommand`]: khora_core::script::WorldCommand
//! [`CommandBuffer`]: khora_core::script::CommandBuffer

mod applied;
pub use applied::AppliedWrites;
mod apply;
mod json;
mod prefab_spawn;

pub use apply::ApplyError;

use crate::ecs::SystemContext;
use khora_core::lane::OutputDeck;
use khora_core::script::CommandBuffer;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

/// Applies every queued command, logging the ones that fail, and returns how
/// many applied.
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
    // The spawns refused so far: a later command naming one of those ids is
    // meant for an entity that never came to be, not a live one holding it.
    // Column writes skip the check: a refused spawn is rolled back, so its id
    // names no live entity (unless a reserver the world does not own gave it).
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
        role: crate::ecs::Role::Serves(khora_core::script::SCRIPT),
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

#[cfg(test)]
mod tests;
