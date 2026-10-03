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

//! Recording live behavior state back into the scene.
//!
//! The counterpart of [`script_commands`]: that one applies what a script asked
//! for, this one applies what the engine observed. Both drain a deck slot at the
//! frame boundary, which is where the `World` may be written.
//!
//! # Why the component has to be current at every moment
//!
//! A save can happen at any frame, and it reads the `World` — so the entity has
//! to already hold what the lane has made of the behavior. Waiting for a "save
//! is coming" signal would put the answer one frame behind the question, and a
//! save is synchronous.
//!
//! # Why a component of its own
//!
//! What the lane observed goes into [`ScriptState`], never into [`Script`]:
//! `Script` is what the author wrote, and a scene file holds it; `ScriptState`
//! is what play made of it, and only a game save holds it. Writing one into
//! the other would make every scene saved after a run carry that run.
//!
//! It is affordable because the lane sends only the instances that spent fuel:
//! a behavior that handled no event ran no code and changed nothing, so a quiet
//! frame writes nothing at all.
//!
//! [`script_commands`]: super::script_commands

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::script::{ScriptSnapshot, ScriptStateWriteback};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, Script, ScriptState, TickPhase, Without, World};

fn script_state_writeback(world: &mut World, _runtime: &Runtime, deck: &mut OutputDeck) {
    if deck.contains::<ScriptStateWriteback>() {
        let mut updates = deck.take::<ScriptStateWriteback>();
        for update in updates.drain() {
            record(world, update.entity, update.behavior, update.snapshot);
        }
    }
    drop_orphaned_states(world);
}

/// Records what the lane observed of `entity`'s behavior, if the entity still
/// runs that behavior: an update for a behavior the entity no longer has is
/// the last word of one that was replaced, and is dropped.
fn record(world: &mut World, entity: EntityId, behavior: String, snapshot: ScriptSnapshot) {
    let runs_it = world
        .get::<Script>(entity)
        .is_some_and(|script| script.behavior == behavior);
    if !runs_it {
        return;
    }
    if let Some(state) = world.get_mut::<ScriptState>(entity) {
        state.behavior = behavior;
        state.snapshot = snapshot;
        return;
    }
    let _ = world.add_component(entity, ScriptState { behavior, snapshot });
}

/// Removes every [`ScriptState`] whose entity no longer has a [`Script`]: what
/// a behavior observed means nothing once the behavior is gone, and a game
/// save would otherwise keep it.
fn drop_orphaned_states(world: &mut World) {
    let orphans: Vec<EntityId> = world
        .query::<(EntityId, &ScriptState, Without<Script>)>()
        .map(|(entity, _, _)| entity)
        .collect();
    for entity in orphans {
        let _ = world.remove_component::<ScriptState>(entity);
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "script_state_writeback",
        phase: TickPhase::Maintenance,
        // After `script_commands` (20): a despawn the script asked for should
        // have happened before its state is written, so the write finds nothing
        // rather than recording state for an entity about to vanish.
        order_hint: 30,
        run: script_state_writeback,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests;
