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

//! What the writeback records, and where: observed state goes to a
//! `ScriptState` beside the authored `Script`, never into it.

use super::*;
use crate::ecs::{Script, ScriptState, Transform};
use crate::scene::{apply, capture_world, Identity};
use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptSnapshot, ScriptStateUpdate, ScriptValue};

fn update(entity: EntityId, behavior: &str, health: i64) -> ScriptStateUpdate {
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

fn run(world: &mut World, deck: &mut OutputDeck) {
    script_state_writeback(world, &Runtime::default(), deck);
}

/// **What a save needs to be true.** What the lane made of the behavior is on
/// the entity, in a `ScriptState` the writeback added, naming the behavior
/// that observed it.
#[test]
fn live_state_reaches_a_script_state_it_adds() {
    let mut world = World::new();
    let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
    assert!(world.get::<ScriptState>(entity).is_none());

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );

    let state = world
        .get::<ScriptState>(entity)
        .expect("the writeback added the observed state");
    assert_eq!(state.behavior, "Guard");
    assert_eq!(state.snapshot.field("health"), Some(&ScriptValue::Int(40)));
}

/// A later frame's update replaces the earlier one: the state is what the lane
/// observed last, not an accumulation.
#[test]
fn a_later_update_replaces_the_state() {
    let mut world = World::new();
    let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );
    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 25)]),
    );

    let state = world.get::<ScriptState>(entity).expect("still recorded");
    assert_eq!(
        state.snapshot,
        ScriptSnapshot::default().with_field("health", ScriptValue::Int(25))
    );
}

/// **The authored half is left alone, all of it.** A designer who typed 100
/// and watched the guard drop to 40 has not changed their mind about where it
/// starts; the `Script` reads exactly as authored after any writeback.
#[test]
fn the_script_is_never_written() {
    let mut world = World::new();
    let authored = Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100));
    let entity = world.spawn(authored.clone());

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );
    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 25)]),
    );

    assert_eq!(world.get::<Script>(entity), Some(&authored));
    assert!(world.get::<ScriptState>(entity).is_some());
}

/// An update names the behavior that produced it. A chest's entity receiving
/// a guard's state would hand the chest a guard's fields on the next load.
#[test]
fn an_update_for_another_behavior_is_not_applied() {
    let mut world = World::new();
    let entity = world.spawn(Script::new("loot/chest.erg", "Chest"));

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );

    assert!(
        world.get::<ScriptState>(entity).is_none(),
        "no state was recorded for a behavior the entity does not run"
    );
}

/// And a state the chest already has is not overwritten by the guard's.
#[test]
fn an_update_for_another_behavior_leaves_the_existing_state() {
    let mut world = World::new();
    let chest_state = ScriptState {
        behavior: "Chest".to_owned(),
        snapshot: ScriptSnapshot::default().with_field("gold", ScriptValue::Int(3)),
    };
    let entity = world.spawn((Script::new("loot/chest.erg", "Chest"), chest_state.clone()));

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );

    assert_eq!(world.get::<ScriptState>(entity), Some(&chest_state));
}

/// An entity whose behavior was swapped keeps the state of the behavior it
/// had until the new one reports; the new one's first update replaces it.
#[test]
fn an_update_replaces_a_state_left_by_another_behavior() {
    let mut world = World::new();
    let entity = world.spawn((
        Script::new("ai/guard.erg", "Chase"),
        ScriptState {
            behavior: "Guard".to_owned(),
            snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(9)),
        },
    ));

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Chase", 40)]),
    );

    let state = world.get::<ScriptState>(entity).expect("recorded");
    assert_eq!(state.behavior, "Chase");
    assert_eq!(state.snapshot.field("health"), Some(&ScriptValue::Int(40)));
}

/// **The invariant the writeback keeps.** Observed state with no behavior to
/// belong to — its `Script` removed by a command or by the editor — is
/// removed, even on a frame no behavior reported anything: otherwise the next
/// save would carry the state of a behavior that is no longer there.
#[test]
fn a_state_whose_entity_lost_its_script_is_removed() {
    let mut world = World::new();
    let orphan = world.spawn((
        Transform::identity(),
        ScriptState {
            behavior: "Guard".to_owned(),
            snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(40)),
        },
    ));
    let kept = world.spawn((
        Script::new("ai/guard.erg", "Guard"),
        ScriptState {
            behavior: "Guard".to_owned(),
            snapshot: ScriptSnapshot::default(),
        },
    ));

    run(&mut world, &mut OutputDeck::new());

    assert!(world.contains(orphan), "the entity itself stays");
    assert!(world.get::<Transform>(orphan).is_some());
    assert!(
        world.get::<ScriptState>(orphan).is_none(),
        "its orphaned state is gone"
    );
    assert!(
        world.get::<ScriptState>(kept).is_some(),
        "a state that still has its behavior is kept"
    );
}

/// An entity despawned earlier the same frame has nowhere to put its state,
/// which is correct — there is no longer anything to save.
#[test]
fn state_for_a_despawned_entity_is_dropped() {
    let mut world = World::new();
    let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
    let other = world.spawn(Script::new("ai/guard.erg", "Guard"));
    world.despawn(entity);

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40), update(other, "Guard", 7)]),
    );

    assert!(!world.contains(entity), "and nothing panicked");
    assert_eq!(
        world
            .get::<ScriptState>(other)
            .and_then(|state| state.snapshot.field("health")),
        Some(&ScriptValue::Int(7)),
        "the live entity's update still lands"
    );
}

/// A frame in which no behavior did work leaves no slot, and the system adds
/// nothing — which is what makes writing back every frame affordable.
#[test]
fn a_quiet_frame_does_nothing() {
    let mut world = World::new();
    let authored = Script::new("ai/guard.erg", "Guard");
    let entity = world.spawn(authored.clone());

    run(&mut world, &mut OutputDeck::new());

    assert_eq!(world.get::<Script>(entity), Some(&authored));
    assert!(world.get::<ScriptState>(entity).is_none());
}

/// The slot is drained, so state from one frame is not re-applied on the
/// next — by then the lane has sent whatever is current.
#[test]
fn the_slot_is_drained() {
    let mut world = World::new();
    let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
    let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

    run(&mut world, &mut deck);
    assert!(!deck.contains::<ScriptStateWriteback>());
}

/// **A scene holds what was authored, never what was observed.** After the
/// lane's state is written back, the scene captured from the world has no
/// `ScriptState` column, and the `Script` it holds loads back exactly as
/// authored.
#[test]
fn a_scene_file_never_holds_observed_state() {
    let mut world = World::new();
    let authored = Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100));
    let entity = world.spawn((Transform::identity(), authored.clone()));
    world.mark_authored(entity).expect("alive");

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );
    assert!(
        world.get::<ScriptState>(entity).is_some(),
        "the observed state is in the world"
    );

    let scene = capture_world(&world).expect("captures");
    assert!(
        scene
            .pages
            .iter()
            .all(|page| page.components.iter().all(|name| name != "ScriptState")),
        "no page of the scene holds a ScriptState: {:?}",
        scene
            .pages
            .iter()
            .map(|page| &page.components)
            .collect::<Vec<_>>()
    );

    let mut loaded = World::new();
    let applied = apply(&mut loaded, &scene, Identity::Keep).expect("loads");
    let (_, back) = applied.entities[0];
    assert_eq!(loaded.get::<Script>(back), Some(&authored));
    assert!(loaded.get::<ScriptState>(back).is_none());
}

/// **A save keeps the lifecycle.** What the lane recorded of whether the
/// instance spawned and of the fault that disabled it reaches the
/// `ScriptState` with the rest of the snapshot.
#[test]
fn the_lifecycle_reaches_the_script_state() {
    use khora_core::script::{InstanceLifecycle, RecordedFault};

    let mut world = World::new();
    let entity = world.spawn(Script::new("ai/guard.erg", "Guard"));
    let lifecycle = InstanceLifecycle {
        resume_failed: Vec::new(),
        spawned: true,
        fault: Some(RecordedFault {
            fingerprint: 0xdead_beef,
            reason: "DivideByZero".to_owned(),
        }),
    };
    let mut recorded = update(entity, "Guard", 40);
    recorded.snapshot.lifecycle = lifecycle.clone();

    run(&mut world, &mut deck_with(vec![recorded]));

    let state = world.get::<ScriptState>(entity).expect("recorded");
    assert_eq!(state.snapshot.lifecycle, lifecycle);
}
