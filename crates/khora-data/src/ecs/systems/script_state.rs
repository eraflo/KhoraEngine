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
//! A save can happen at any frame, and it reads the `World` — so the `Script`
//! component has to already hold what the lane has made of the behavior. Waiting
//! for a "save is coming" signal would put the answer one frame behind the
//! question, and the editor's save is synchronous.
//!
//! It is affordable because the lane sends only the instances that spent fuel:
//! a behavior that handled no event ran no code and changed nothing, so a quiet
//! frame writes nothing at all.
//!
//! [`script_commands`]: super::script_commands

use khora_core::lane::OutputDeck;
use khora_core::script::ScriptStateWriteback;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, Script, TickPhase, World};

fn script_state_writeback(world: &mut World, _runtime: &Runtime, deck: &mut OutputDeck) {
    if !deck.contains::<ScriptStateWriteback>() {
        return;
    }
    let mut updates = deck.take::<ScriptStateWriteback>();

    for update in updates.drain() {
        let Some(script) = world.get_mut::<Script>(update.entity) else {
            // The entity was despawned by a command applied earlier this same
            // frame. Its state has nowhere to go, which is correct — there is
            // no longer anything to save.
            continue;
        };
        // An entity may carry several behaviors, and this update belongs to one
        // of them. Writing without checking would let a guard's fields land in
        // the chest component sharing its entity.
        if script.behavior != update.behavior {
            continue;
        }
        // Only the observed half. `fields` is the designer's authored starting
        // values, and overwriting those with what the game has since made of
        // them would erase an edit the moment the entity ran once.
        script.runtime = update.snapshot;
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
mod tests {
    use super::*;
    use khora_core::script::{ScriptSnapshot, ScriptStateUpdate, ScriptValue};

    fn update(
        entity: khora_core::ecs::entity::EntityId,
        behavior: &str,
        health: i64,
    ) -> ScriptStateUpdate {
        ScriptStateUpdate {
            entity,
            behavior: behavior.to_owned(),
            snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(health)),
        }
    }

    fn deck_with(updates: Vec<ScriptStateUpdate>) -> OutputDeck {
        let mut deck = OutputDeck::new();
        deck.slot::<ScriptStateWriteback>().extend(updates);
        deck
    }

    /// **What a save needs to be true.** The component holds what the lane has
    /// made of the behavior, not only what its author typed.
    #[test]
    fn live_state_reaches_the_component() {
        let mut world = World::new();
        let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
        let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

        script_state_writeback(&mut world, &Runtime::default(), &mut deck);

        assert_eq!(
            world
                .get::<Script>(entity)
                .and_then(|s| s.runtime.field("health")),
            Some(&ScriptValue::Int(40))
        );
    }

    /// **And the authored half is left alone.** A designer who typed 100 and
    /// then watched the guard drop to 40 has not changed their mind about where
    /// it starts; overwriting `fields` would erase the edit the moment the
    /// entity ran once, and the next reset would begin at 40.
    #[test]
    fn the_authored_values_are_not_overwritten() {
        let mut world = World::new();
        let entity = world.spawn(
            Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100)),
        );
        let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

        script_state_writeback(&mut world, &Runtime::default(), &mut deck);

        let script = world.get::<Script>(entity).expect("still there");
        assert_eq!(script.field("health"), Some(&ScriptValue::Int(100)));
        assert_eq!(script.runtime.field("health"), Some(&ScriptValue::Int(40)));
    }

    /// An entity may carry several behaviors, and an update belongs to one.
    /// Writing without checking would let a guard's fields land in the chest
    /// sharing its entity.
    #[test]
    fn an_update_for_another_behavior_is_not_applied() {
        let mut world = World::new();
        let entity = world.spawn(Script::new("loot/chest.erg", "Chest"));
        let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

        script_state_writeback(&mut world, &Runtime::default(), &mut deck);

        assert_eq!(
            world.get::<Script>(entity).map(|s| s.runtime.is_empty()),
            Some(true),
            "the chest kept its own state"
        );
    }

    /// An entity despawned earlier the same frame has nowhere to put its state,
    /// which is correct — there is no longer anything to save.
    #[test]
    fn state_for_a_despawned_entity_is_dropped() {
        let mut world = World::new();
        let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
        world.despawn(entity);

        let mut deck = deck_with(vec![update(entity, "Guard", 40)]);
        script_state_writeback(&mut world, &Runtime::default(), &mut deck);

        assert!(!world.contains(entity), "and nothing panicked");
    }

    /// A frame in which no behavior did work leaves no slot, and the system is
    /// a no-op — which is what makes writing back every frame affordable.
    #[test]
    fn a_quiet_frame_does_nothing() {
        let mut world = World::new();
        let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
        let mut deck = OutputDeck::new();

        script_state_writeback(&mut world, &Runtime::default(), &mut deck);

        assert_eq!(world.get::<Script>(entity).map(|s| s.fields.len()), Some(0));
    }

    /// The slot is drained, so state from one frame is not re-applied on the
    /// next — by then the lane has sent whatever is current.
    #[test]
    fn the_slot_is_drained() {
        let mut world = World::new();
        let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
        let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

        script_state_writeback(&mut world, &Runtime::default(), &mut deck);
        assert!(!deck.contains::<ScriptStateWriteback>());
    }
}
