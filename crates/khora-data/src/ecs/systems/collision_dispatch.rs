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

//! Moves this frame's contacts from the physics lane's [`ContactBatch`] deck
//! slot into the [`Channel<Collision>`](khora_core::event::Channel).
//!
//! A contact relates two entities, so it is no component; and it is lane
//! output, not a callback from inside the step, so its readers run under
//! their own budget.

use crate::ecs::SystemContext;
use khora_core::event::Channel;
use khora_core::lane::OutputDeck;
use khora_core::physics::{Collision, ContactBatch};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn collision_dispatch(
    _world: &mut World,
    runtime: &Runtime,
    deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    if !deck.contains::<ContactBatch>() {
        // The lane did not run (paused, or no provider): that is not a frame
        // with no contacts, and taking the slot would report it as one.
        return;
    }
    let batch = deck.take::<ContactBatch>();
    if batch.contacts.is_empty() {
        return;
    }

    let Some(collisions) = runtime.resources.get::<Channel<Collision>>() else {
        // A host with no physics consumer: the contacts are dropped.
        return;
    };
    for contact in batch.contacts {
        collisions.send(contact);
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "collision_dispatch",
        phase: TickPhase::Maintenance,
        run: collision_dispatch,
        // After `physics_world_writeback` (0), so a consumer that reacts to a
        // contact next frame sees the transforms the step that produced it
        // ended at, rather than the ones it started from.
        order_hint: 10,
        runs_after: &[],
        observes: &[],
        role: crate::ecs::Role::Serves(khora_core::physics::PHYSICS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;
    use khora_core::physics::{collision_channel, CollisionKind};

    fn entity(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 0,
        }
    }

    fn a_contact() -> Collision {
        Collision {
            kind: CollisionKind::Started,
            a: entity(1),
            b: entity(2),
        }
    }

    fn wired() -> (Runtime, Channel<Collision>) {
        let collisions = collision_channel();
        let mut runtime = Runtime::default();
        runtime.resources.insert(collisions.clone());
        (runtime, collisions)
    }

    fn run(runtime: &Runtime, deck: &mut OutputDeck) {
        collision_dispatch(
            &mut World::new(),
            runtime,
            deck,
            crate::ecs::SystemContext {
                since: crate::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
    }

    #[test]
    fn a_contact_on_the_deck_reaches_the_channel() {
        let (runtime, collisions) = wired();
        let mut deck = OutputDeck::new();
        deck.slot::<ContactBatch>().contacts.push(a_contact());

        run(&runtime, &mut deck);

        assert_eq!(collisions.len(), 1);
    }

    /// **A lane that did not run is not a frame with no contacts**: its slot is
    /// absent, not empty, and a consumer counting quiet frames tells them apart.
    #[test]
    fn a_frame_where_physics_did_not_run_reports_nothing() {
        let (runtime, collisions) = wired();
        let mut deck = OutputDeck::new();

        run(&runtime, &mut deck);

        assert!(collisions.is_empty());
        assert_eq!(collisions.dropped(), 0);
    }

    /// Taken, not read: the deck is the lane's output for this frame, and a
    /// second reader of it would be a second dispatch of the same contacts.
    #[test]
    fn the_slot_is_taken_so_a_contact_is_dispatched_once() {
        let (runtime, collisions) = wired();
        let mut deck = OutputDeck::new();
        deck.slot::<ContactBatch>().contacts.push(a_contact());

        run(&runtime, &mut deck);
        run(&runtime, &mut deck);

        assert_eq!(collisions.len(), 1, "the second run found nothing to send");
    }

    /// A host with no consumer still runs. Physics is not obliged to have
    /// somebody listening.
    #[test]
    fn a_runtime_with_no_channel_runs_without_complaint() {
        let mut deck = OutputDeck::new();
        deck.slot::<ContactBatch>().contacts.push(a_contact());

        run(&Runtime::default(), &mut deck);
    }
}
