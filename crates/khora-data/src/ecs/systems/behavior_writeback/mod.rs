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
//! # Why two components
//!
//! What a turn changed of the behavior's fields goes into the behavior's own
//! component — the fields a scene holds, edited by the game as it plays —
//! and what the engine keeps to resume the instance goes into its state
//! companion, `Guard#state`, which only a game save holds. Writing the
//! engine's state into the authored component would make every scene saved
//! after a run carry that run.
//!
//! It is affordable because the lane sends only the instances that spent fuel:
//! a behavior that handled no event ran no code and changed nothing, so a quiet
//! frame writes nothing at all.
//!
//! [`script_commands`]: super::script_commands

use khora_core::lane::OutputDeck;
use khora_core::script::BehaviorWriteback;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn behavior_writeback(world: &mut World, _runtime: &Runtime, deck: &mut OutputDeck) {
    // What the frame's commands wrote, and where: a turn's field a command
    // written after it overwrote is that command's.
    let applied = deck.take::<super::script_commands::AppliedWrites>();
    if deck.contains::<BehaviorWriteback>() {
        let mut updates = deck.take::<BehaviorWriteback>();
        attached::write_back(world, updates.drain().collect(), &applied);
    }
    attached::drop_orphaned_companions(world);
}

inventory::submit! {
    DataSystemRegistration {
        name: "behavior_writeback",
        phase: TickPhase::Maintenance,
        // After `script_commands` (20): a despawn the script asked for should
        // have happened before its state is written, so the write finds nothing
        // rather than recording state for an entity about to vanish.
        order_hint: 30,
        run: behavior_writeback,
        runs_after: &[],
        observes: &[],
    }
}

mod attached;
mod order;
#[cfg(test)]
mod tests;
