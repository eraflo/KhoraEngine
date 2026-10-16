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

//! What the writeback records, and where: a turn's changed fields go to the
//! behavior's component, what the engine keeps of the instance to its state
//! companion, `Guard#state`.

use super::*;
use crate::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    ScriptRole, SemanticDomain, Transform,
};
use crate::flow::companion::companion_snapshot;
use crate::scene::capture_world;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{BehaviorUpdate, ScriptSnapshot, ScriptValue};

mod emission_order;

fn field(name: &str, kind: FieldKind, default: ScriptValue) -> PackedField {
    PackedField {
        name: name.to_owned(),
        kind,
        default,
    }
}

fn register(
    world: &mut World,
    name: &str,
    provenance: ComponentProvenance,
    resumable: bool,
    fields: Vec<PackedField>,
) -> ComponentKey {
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
            domain: SemanticDomain::Script,
            provenance,
            resumable,
            layout: PackedLayout::new(fields).expect("a valid layout"),
        })
        .unwrap_or_else(|error| panic!("`{name}` registers: {error}"))
}

/// Registers the behavior `name` that `module` declares — `int` fields
/// `health` and `armor` — and its state companion, as the scripts'
/// registration does. Returns both keys.
fn declare_behavior(world: &mut World, module: &str, name: &str) -> (ComponentKey, ComponentKey) {
    let behavior = register(
        world,
        name,
        ComponentProvenance::Authored,
        false,
        vec![
            field("health", FieldKind::Int, ScriptValue::Int(0)),
            field("armor", FieldKind::Int, ScriptValue::Int(0)),
        ],
    );
    world.set_script_role(
        behavior,
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order: 0,
        }),
    );
    let companion = register(
        world,
        &format!("{name}#state"),
        ComponentProvenance::Runtime,
        true,
        vec![
            field("state", FieldKind::Value, ScriptValue::Null),
            field(
                "state_fields",
                FieldKind::Value,
                ScriptValue::Struct(Vec::new()),
            ),
            field("timers", FieldKind::Value, ScriptValue::Array(Vec::new())),
            field("pending", FieldKind::Value, ScriptValue::Null),
            field("spawned", FieldKind::Bool, ScriptValue::Bool(false)),
            field("fault", FieldKind::Value, ScriptValue::Null),
            field(
                "resume_failed",
                FieldKind::Value,
                ScriptValue::Array(Vec::new()),
            ),
            field("owed", FieldKind::Float, ScriptValue::Float(0.0)),
            field(
                "fixed_owed",
                FieldKind::Value,
                ScriptValue::Array(Vec::new()),
            ),
        ],
    );
    world.set_script_role(
        companion,
        Some(ScriptRole::BehaviorState {
            behavior: name.to_owned(),
        }),
    );
    (behavior, companion)
}

/// An entity running behavior `key`, its `health` and `armor` as authored.
fn running(world: &mut World, key: ComponentKey, health: i64, armor: i64) -> EntityId {
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![
                ("health".to_owned(), ScriptValue::Int(health)),
                ("armor".to_owned(), ScriptValue::Int(armor)),
            ]),
        )
        .expect("the behavior attaches");
    entity
}

/// `entity`'s value of field `name` of component `key`.
fn read(world: &World, entity: EntityId, key: ComponentKey, name: &str) -> Option<ScriptValue> {
    let slot = world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(name)?;
    world.row(entity, key)?.field(slot)
}

/// What `entity`'s state companion `key` records, if it holds one.
fn recorded(world: &World, entity: EntityId, key: ComponentKey) -> Option<ScriptSnapshot> {
    let layout = world.components().vtable(key)?.columns.packed()?;
    let row = world.row(entity, key)?;
    let fields: Vec<(String, ScriptValue)> = layout
        .fields()
        .iter()
        .enumerate()
        .map(|(slot, field)| {
            (
                field.name.clone(),
                row.field(slot).expect("every field reads"),
            )
        })
        .collect();
    let snapshot = companion_snapshot(&fields).expect("the companion reads back");
    Some(snapshot)
}

/// A turn of `behavior` on `entity` that changed its `health` to `health`.
fn update(entity: EntityId, behavior: &str, health: i64) -> BehaviorUpdate {
    BehaviorUpdate {
        entity,
        behavior: behavior.to_owned(),
        snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(health)),
        changed: vec!["health".to_owned()],
        read_until: 0,
        emitted_after: 0,
    }
}

fn deck_with(updates: Vec<BehaviorUpdate>) -> OutputDeck {
    let mut deck = OutputDeck::new();
    deck.slot::<BehaviorWriteback>().extend(updates);
    deck
}

fn run(world: &mut World, deck: &mut OutputDeck) {
    behavior_writeback(world, &Runtime::default(), deck);
}

/// **What a save needs to be true.** A turn's changed field is on the
/// behavior's component, and what the engine keeps of the instance is in a
/// state companion the writeback added.
#[test]
fn a_turn_reaches_the_component_and_a_companion_it_adds() {
    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);
    assert!(world.row(entity, state).is_none());

    let mut turn = update(entity, "Guard", 40);
    turn.snapshot.state = Some("Alert".to_owned());
    run(&mut world, &mut deck_with(vec![turn]));

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(40))
    );
    let recorded = recorded(&world, entity, state).expect("the writeback added the companion");
    assert_eq!(recorded.state.as_deref(), Some("Alert"));
}

/// Only the fields a turn changed are written: another field keeps what it
/// holds, whatever the snapshot says of it.
#[test]
fn only_the_changed_fields_are_written() {
    let mut world = World::new();
    let (guard, _) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);

    let mut turn = update(entity, "Guard", 40);
    turn.snapshot = turn.snapshot.with_field("armor", ScriptValue::Int(0));
    run(&mut world, &mut deck_with(vec![turn]));

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(40))
    );
    assert_eq!(
        read(&world, entity, guard, "armor"),
        Some(ScriptValue::Int(5)),
        "a field the turn did not change keeps its value"
    );
}

/// A later frame's update replaces the earlier one: the companion records
/// what the lane observed last, not an accumulation.
#[test]
fn a_later_update_replaces_the_companion() {
    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);

    let mut first = update(entity, "Guard", 40);
    first.snapshot.state = Some("Alert".to_owned());
    run(&mut world, &mut deck_with(vec![first]));
    let mut second = update(entity, "Guard", 25);
    second.snapshot.state = Some("Patrol".to_owned());
    run(&mut world, &mut deck_with(vec![second]));

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(25))
    );
    assert_eq!(
        recorded(&world, entity, state)
            .expect("still recorded")
            .state
            .as_deref(),
        Some("Patrol")
    );
}

/// An update names the behavior that produced it. An entity that does not
/// hold that behavior receives nothing of it — neither its fields nor a
/// companion.
#[test]
fn an_update_for_a_behavior_the_entity_does_not_hold_is_not_applied() {
    let mut world = World::new();
    let (_, guard_state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let (chest, chest_state) = declare_behavior(&mut world, "loot/chest.erg", "Chest");
    let entity = running(&mut world, chest, 3, 0);

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );

    assert_eq!(
        read(&world, entity, chest, "health"),
        Some(ScriptValue::Int(3))
    );
    assert!(world.row(entity, guard_state).is_none());
    assert!(world.row(entity, chest_state).is_none());
}

/// **The invariant the writeback keeps.** A companion whose entity no longer
/// holds its behavior — removed by a command or by the editor — is removed,
/// even on a frame no behavior reported anything: a behavior attached again
/// starts afresh.
#[test]
fn a_companion_whose_entity_lost_its_behavior_is_removed() {
    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let orphan = running(&mut world, guard, 100, 5);
    let kept = running(&mut world, guard, 100, 5);
    run(
        &mut world,
        &mut deck_with(vec![update(orphan, "Guard", 40), update(kept, "Guard", 30)]),
    );
    assert!(world.row(orphan, state).is_some());
    assert!(world.remove_component_by_key(orphan, guard));

    run(&mut world, &mut OutputDeck::new());

    assert!(world.contains(orphan), "the entity itself stays");
    assert!(world.get::<Transform>(orphan).is_some());
    assert!(
        world.row(orphan, state).is_none(),
        "its orphaned companion is gone"
    );
    assert!(
        world.row(kept, state).is_some(),
        "a companion that still has its behavior is kept"
    );
}

/// An entity despawned earlier the same frame has nowhere to put its state,
/// which is correct — there is no longer anything to save.
#[test]
fn an_update_for_a_despawned_entity_is_dropped() {
    let mut world = World::new();
    let (guard, _) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let gone = running(&mut world, guard, 100, 5);
    let other = running(&mut world, guard, 100, 5);
    world.despawn(gone);

    run(
        &mut world,
        &mut deck_with(vec![update(gone, "Guard", 40), update(other, "Guard", 7)]),
    );

    assert!(!world.contains(gone), "and nothing panicked");
    assert_eq!(
        read(&world, other, guard, "health"),
        Some(ScriptValue::Int(7)),
        "the live entity's update still lands"
    );
}

/// A frame in which no behavior did work leaves no slot, and the system
/// writes nothing — which is what makes writing back every frame affordable.
#[test]
fn a_quiet_frame_writes_nothing() {
    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);

    run(&mut world, &mut OutputDeck::new());

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(100))
    );
    assert!(world.row(entity, state).is_none(), "no companion was added");
}

/// The slot is drained, so a frame's updates are not applied again on the
/// next — by then the lane has sent whatever is current.
#[test]
fn the_slot_is_drained() {
    let mut world = World::new();
    let (guard, _) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);
    let mut deck = deck_with(vec![update(entity, "Guard", 40)]);

    run(&mut world, &mut deck);
    assert!(!deck.contains::<BehaviorWriteback>());
}

/// **A scene holds what was authored, never what the engine kept.** After a
/// writeback, the scene captured from the world holds the behavior's
/// component and no page holds its state companion.
#[test]
fn a_scene_never_holds_the_companion() {
    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);
    world.mark_authored(entity).expect("alive");

    run(
        &mut world,
        &mut deck_with(vec![update(entity, "Guard", 40)]),
    );
    assert!(
        world.row(entity, state).is_some(),
        "the companion is in the world"
    );

    let scene = capture_world(&world).expect("captures");
    let names: Vec<&String> = scene
        .pages
        .iter()
        .flat_map(|page| &page.components)
        .collect();
    assert!(names.iter().any(|name| *name == "Guard"), "{names:?}");
    assert!(names.iter().all(|name| *name != "Guard#state"), "{names:?}");
}

/// **A save keeps the lifecycle.** What the lane recorded of whether the
/// instance spawned and of the fault that disabled it reaches the companion
/// with the rest of the snapshot.
#[test]
fn the_lifecycle_reaches_the_companion() {
    use khora_core::script::{InstanceLifecycle, RecordedFault};

    let mut world = World::new();
    let (guard, state) = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = running(&mut world, guard, 100, 5);
    let lifecycle = InstanceLifecycle {
        owed: 0.0,
        fixed_owed: Vec::new(),
        resume_failed: Vec::new(),
        spawned: true,
        fault: Some(RecordedFault {
            fingerprint: 0xdead_beef,
            reason: "DivideByZero".to_owned(),
        }),
    };
    let mut turn = update(entity, "Guard", 40);
    turn.snapshot.lifecycle = lifecycle.clone();

    run(&mut world, &mut deck_with(vec![turn]));

    let recorded = recorded(&world, entity, state).expect("recorded");
    assert_eq!(recorded.lifecycle, lifecycle);
}
