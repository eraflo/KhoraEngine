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

//! Writes what the script lane made of each running behavior back to the
//! World: its changed fields, and its state companion.
//!
//! A save may read the World at any frame, so this runs every frame; the lane
//! sends only the instances that spent fuel, so a quiet frame writes nothing.
//! See the book: concepts/scripting/saves-and-edits.md, "Saved mid-sentence".

use crate::ecs::SystemContext;
use khora_core::lane::OutputDeck;
use khora_core::script::BehaviorWriteback;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn behavior_writeback(
    world: &mut World,
    _runtime: &Runtime,
    deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
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
        // After `script_commands` (20): a despawn a script asked for lands
        // first, so no state is recorded for an entity about to vanish.
        order_hint: 30,
        run: behavior_writeback,
        runs_after: &[],
        observes: &[],
        role: crate::ecs::Role::Serves(khora_core::script::SCRIPT),
    }
}

mod attached;
mod order;

#[cfg(test)]
mod tests;
