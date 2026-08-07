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

//! Getting this frame's contacts from the deck to whoever listens.
//!
//! The physics lane resolves each contact to two entities and leaves them in a
//! [`ContactBatch`] deck slot; this drains that slot into the
//! [`Channel<Collision>`](khora_core::event::Channel) any number of consumers
//! subscribe to.
//!
//! # Why not a component
//!
//! Collisions used to be a `CollisionEvents` component, and being one cost more
//! than it looked. It was **serialized into scene files** — the derive took the
//! default `Authored` provenance, so a saved scene recorded last frame's
//! contacts and the editor offered them under "+ Add Component" as editable
//! JSON. Every entity carrying it received the identical global list rather
//! than its own contacts. And nothing ever attached it, so every contact Rapier
//! reported was drained and dropped, every frame.
//!
//! Underneath all three: a collision is not a property of an entity. It is a
//! relation between two, and putting it on one forces a choice of which or a
//! duplication across both.
//!
//! # Why not a callback
//!
//! Unity, Unreal and Godot deliver contacts by calling game code from inside
//! the physics step — reentrant, ordered by nothing in particular, and
//! impossible to budget, since the engine cannot know what the notification
//! will cost before it runs. Khora's answer is that a contact is lane output
//! like any other: it lands on the deck, a data system moves it, and what reads
//! it does so under a budget it negotiated.

use khora_core::event::Channel;
use khora_core::lane::OutputDeck;
use khora_core::physics::{Collision, ContactBatch};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn collision_dispatch(_world: &mut World, runtime: &Runtime, deck: &mut OutputDeck) {
    if !deck.contains::<ContactBatch>() {
        // The lane did not run this frame — paused, or no provider. Not the
        // same as "no contacts", and taking the slot would report an empty
        // frame of physics as a frame of physics with nothing in it.
        return;
    }
    let batch = deck.take::<ContactBatch>();
    if batch.contacts.is_empty() {
        return;
    }

    let Some(collisions) = runtime.resources.get::<Channel<Collision>>() else {
        // No channel installed: a host that does not run physics consumers.
        // The contacts are dropped here rather than accumulating in the deck,
        // which is fresh every frame anyway.
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
        collision_dispatch(&mut World::new(), runtime, deck);
    }

    #[test]
    fn a_contact_on_the_deck_reaches_the_channel() {
        let (runtime, collisions) = wired();
        let mut deck = OutputDeck::new();
        deck.slot::<ContactBatch>().contacts.push(a_contact());

        run(&runtime, &mut deck);

        assert_eq!(collisions.len(), 1);
    }

    /// **The distinction the old component could not make.** A lane that did
    /// not run is not a frame with no contacts: one leaves the slot absent, the
    /// other leaves it empty, and a consumer counting quiet frames needs to
    /// tell them apart.
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
