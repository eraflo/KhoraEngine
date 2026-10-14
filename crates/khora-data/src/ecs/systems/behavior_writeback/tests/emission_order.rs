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

//! A turn's field changes and the scripts' `Set`s of the same fields, in the
//! order they were made.
//!
//! The frame boundary runs `script_commands`, then `behavior_writeback`, over
//! one deck. A `Set` of a behavior's field queued before the turn
//! (`BehaviorUpdate::emitted_after`) was read by the turn: the turn's value is
//! the later word, and nothing was lost. A `Set` queued at the turn's position
//! or after it, and applied, is the later word: the turn's value of that field
//! is dropped and the loss reported — "the last write wins, the others are
//! lost". A `Set` refused, or a turn value its field refuses, takes nothing
//! from the other. A behavior removed or its entity despawned after the turn
//! receives nothing of it.

use std::cell::RefCell;
use std::sync::Once;

use super::*;
use crate::ecs::{
    ComponentKey, ComponentProvenance, DataSystemRegistration, FieldKind, PackedField,
    PackedLayout, RuntimeComponentDecl, ScriptRole, SemanticDomain, Transform,
};
use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    BehaviorUpdate, CommandBuffer, ComponentName, ScriptSnapshot, ScriptValue, WorldCommand,
};

// ─── What the boundary reported ─────────────────────────────────────────────

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The warnings logged on the thread that logged them, so tests running side
/// by side do not read each other's.
struct Warnings;

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let line = record.args().to_string();
            WARNINGS.with(|warnings| warnings.borrow_mut().push(line));
        }
    }

    fn flush(&self) {}
}

static LOGGER: Warnings = Warnings;

/// Starts recording this thread's warnings afresh.
fn record_warnings() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&LOGGER).expect("no other logger in this test binary");
        log::set_max_level(log::LevelFilter::Warn);
    });
    WARNINGS.with(|warnings| warnings.borrow_mut().clear());
}

/// The lost writes reported since [`record_warnings`].
fn lost_writes() -> Vec<String> {
    WARNINGS.with(|warnings| {
        warnings
            .borrow()
            .iter()
            .filter(|line| line.contains("the last write wins"))
            .cloned()
            .collect()
    })
}

/// Whether `report` names `field` of `entity`'s `Guard`.
fn names(report: &str, entity: EntityId, field: &str) -> bool {
    report.contains(&format!("{}v{}", entity.index, entity.generation))
        && report.contains("Guard")
        && report.contains(field)
}

// ─── A world running `Guard` ────────────────────────────────────────────────

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

/// Registers behavior `Guard` — `int health`, `float speed` — and its state
/// companion, as the scripts' registration does.
fn declare_guard(world: &mut World) -> (ComponentKey, ComponentKey) {
    let guard = register(
        world,
        "Guard",
        ComponentProvenance::Authored,
        false,
        vec![
            field("health", FieldKind::Int, ScriptValue::Int(0)),
            field("speed", FieldKind::Float, ScriptValue::Float(0.0)),
        ],
    );
    world.set_script_role(
        guard,
        Some(ScriptRole::Behavior {
            module: "ai/guard.erg".to_owned(),
            order: 0,
        }),
    );
    let companion = register(
        world,
        "Guard#state",
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
        ],
    );
    world.set_script_role(
        companion,
        Some(ScriptRole::BehaviorState {
            behavior: "Guard".to_owned(),
        }),
    );
    (guard, companion)
}

/// An entity running `Guard`, its `health` 100 and its `speed` 1.
fn running(world: &mut World, guard: ComponentKey) -> EntityId {
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            guard,
            &ScriptValue::Struct(vec![
                ("health".to_owned(), ScriptValue::Int(100)),
                ("speed".to_owned(), ScriptValue::Float(1.0)),
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

/// A turn of `entity`'s `Guard` that changed `fields`, its writes taking
/// effect after the first `emitted_after` commands of the frame.
fn turn(entity: EntityId, fields: &[(&str, ScriptValue)], emitted_after: usize) -> BehaviorUpdate {
    let mut snapshot = ScriptSnapshot::default();
    for (name, value) in fields {
        snapshot = snapshot.with_field(*name, value.clone());
    }
    BehaviorUpdate {
        entity,
        behavior: "Guard".to_owned(),
        snapshot,
        changed: fields.iter().map(|(name, _)| (*name).to_owned()).collect(),
        read_until: emitted_after,
        emitted_after,
    }
}

/// A script's `entity.Set(Guard { … })`.
fn set(entity: EntityId, fields: &[(&str, ScriptValue)]) -> WorldCommand {
    WorldCommand::SetComponent {
        entity,
        component: ComponentName::new("Guard"),
        value: ScriptValue::Struct(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        ),
    }
}

fn system(name: &str) -> fn(&mut World, &Runtime, &mut OutputDeck) {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
        .run
}

/// The frame boundary over a deck holding `commands` and `turns`, as the
/// engine runs it: the scripts' commands, then the writeback. Returns the
/// lost writes it reported.
fn boundary(
    world: &mut World,
    commands: Vec<WorldCommand>,
    turns: Vec<BehaviorUpdate>,
) -> Vec<String> {
    let mut deck = OutputDeck::new();
    deck.slot::<CommandBuffer>().extend(commands);
    deck.slot::<BehaviorWriteback>().extend(turns);
    record_warnings();
    let runtime = Runtime::default();
    system("script_commands")(world, &runtime, &mut deck);
    system("behavior_writeback")(world, &runtime, &mut deck);
    lost_writes()
}

// ─── Before or after the turn ───────────────────────────────────────────────

/// **The turn's position decides.** A `Set` queued before the turn was read
/// by it: the turn's value stands and nothing was lost. A `Set` queued at the
/// turn's position or after it is the last word: it stands, and the turn's
/// lost value is reported, once.
#[test]
fn a_set_before_the_turn_is_read_and_a_set_after_it_wins() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let read_it = running(&mut world, guard);
    let lost_it = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![
            set(read_it, &[("health", ScriptValue::Int(7))]),
            set(lost_it, &[("health", ScriptValue::Int(9))]),
        ],
        vec![
            // Read 7, then took one off.
            turn(read_it, &[("health", ScriptValue::Int(6))], 1),
            turn(lost_it, &[("health", ScriptValue::Int(40))], 1),
        ],
    );

    assert_eq!(
        read(&world, read_it, guard, "health"),
        Some(ScriptValue::Int(6)),
        "the turn read the `Set` and wrote after it"
    );
    assert_eq!(
        read(&world, lost_it, guard, "health"),
        Some(ScriptValue::Int(9)),
        "the `Set` came after the turn"
    );
    assert_eq!(reports.len(), 1, "one lost write: {reports:?}");
    assert!(
        names(&reports[0], lost_it, "health"),
        "the report names the entity, the behavior and the field: {reports:?}"
    );
}

/// A later `Set` takes only the fields it names: the turn's other changes
/// land, and only the field it lost is reported.
#[test]
fn a_later_set_wins_only_the_fields_it_names() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let entity = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![set(entity, &[("speed", ScriptValue::Float(2.0))])],
        vec![turn(
            entity,
            &[
                ("health", ScriptValue::Int(40)),
                ("speed", ScriptValue::Float(9.0)),
            ],
            0,
        )],
    );

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(40))
    );
    assert_eq!(
        read(&world, entity, guard, "speed"),
        Some(ScriptValue::Float(2.0))
    );
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(names(&reports[0], entity, "speed"), "{reports:?}");
    assert!(!reports[0].contains("health"), "{reports:?}");
}

/// A `Set` of a field the turn did not change lands beside the turn's:
/// nothing is lost, nothing reported.
#[test]
fn a_set_of_a_field_the_turn_did_not_change_lands_beside_it() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let entity = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![set(entity, &[("speed", ScriptValue::Float(2.0))])],
        vec![turn(entity, &[("health", ScriptValue::Int(40))], 0)],
    );

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(40))
    );
    assert_eq!(
        read(&world, entity, guard, "speed"),
        Some(ScriptValue::Float(2.0))
    );
    assert!(reports.is_empty(), "{reports:?}");
}

// ─── What was refused takes nothing ─────────────────────────────────────────

/// **Only a write that applied wins.** A `Set` after the turn that its
/// component refused writes nothing: the turn's value stands and is not
/// reported lost — while an applied one beside it wins.
#[test]
fn a_refused_set_after_the_turn_takes_nothing_from_it() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let refused = running(&mut world, guard);
    let applied = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![
            set(refused, &[("health", ScriptValue::Str("seven".to_owned()))]),
            set(applied, &[("health", ScriptValue::Int(9))]),
        ],
        vec![
            turn(refused, &[("health", ScriptValue::Int(40))], 0),
            turn(applied, &[("health", ScriptValue::Int(40))], 0),
        ],
    );

    assert_eq!(
        read(&world, refused, guard, "health"),
        Some(ScriptValue::Int(40)),
        "the refused `Set` wrote nothing; the turn's value stands"
    );
    assert_eq!(
        read(&world, applied, guard, "health"),
        Some(ScriptValue::Int(9))
    );
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(names(&reports[0], applied, "health"), "{reports:?}");
}

/// A turn value its field refuses — a `speed` computed infinite — leaves the
/// `Set` the turn read as it stands, and is no lost write; the turn's field
/// a later `Set` took is.
#[test]
fn a_turn_value_its_field_refuses_leaves_the_set_it_read() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let entity = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![
            set(entity, &[("speed", ScriptValue::Float(2.0))]),
            set(entity, &[("health", ScriptValue::Int(7))]),
        ],
        vec![turn(
            entity,
            &[
                ("speed", ScriptValue::Float(f32::INFINITY)),
                ("health", ScriptValue::Int(40)),
            ],
            1,
        )],
    );

    assert_eq!(
        read(&world, entity, guard, "speed"),
        Some(ScriptValue::Float(2.0)),
        "the turn's `speed` was refused; the `Set` it read stands"
    );
    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(7)),
        "the `Set` after the turn wins"
    );
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(names(&reports[0], entity, "health"), "{reports:?}");
    assert!(!reports[0].contains("speed"), "{reports:?}");
}

// ─── Gone after the turn ────────────────────────────────────────────────────

/// A behavior removed after the turn receives nothing of it, and its state
/// companion goes with it.
#[test]
fn a_behavior_removed_after_the_turn_receives_nothing_and_loses_its_companion() {
    let mut world = World::new();
    let (guard, companion) = declare_guard(&mut world);
    let entity = running(&mut world, guard);
    boundary(
        &mut world,
        Vec::new(),
        vec![turn(entity, &[("health", ScriptValue::Int(50))], 0)],
    );
    assert!(world.row(entity, companion).is_some(), "the premise");

    let reports = boundary(
        &mut world,
        vec![WorldCommand::RemoveComponent {
            entity,
            component: ComponentName::new("Guard"),
        }],
        vec![turn(entity, &[("health", ScriptValue::Int(40))], 0)],
    );

    assert!(world.contains(entity));
    assert!(world.row(entity, guard).is_none(), "nothing put it back");
    assert!(world.row(entity, companion).is_none(), "its companion went");
    assert!(reports.is_empty(), "{reports:?}");
}

/// A behavior removed and added again after the turn is a fresh one: it
/// holds what the `Add` gave it, never the turn of the one removed.
#[test]
fn a_behavior_added_again_after_the_turn_holds_what_the_add_gave_it() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let entity = running(&mut world, guard);

    boundary(
        &mut world,
        vec![
            WorldCommand::RemoveComponent {
                entity,
                component: ComponentName::new("Guard"),
            },
            WorldCommand::AddComponent {
                entity,
                component: ComponentName::new("Guard"),
                value: ScriptValue::Struct(vec![("health".to_owned(), ScriptValue::Int(1))]),
            },
        ],
        vec![turn(
            entity,
            &[
                ("health", ScriptValue::Int(40)),
                ("speed", ScriptValue::Float(9.0)),
            ],
            0,
        )],
    );

    assert_eq!(
        read(&world, entity, guard, "health"),
        Some(ScriptValue::Int(1))
    );
    assert_eq!(
        read(&world, entity, guard, "speed"),
        Some(ScriptValue::Float(0.0)),
        "the field the `Add` left out is at its default"
    );
}

/// An entity despawned after the turn receives nothing, and nothing is
/// reported lost.
#[test]
fn an_entity_despawned_after_the_turn_receives_nothing() {
    let mut world = World::new();
    let (guard, _) = declare_guard(&mut world);
    let gone = running(&mut world, guard);
    let kept = running(&mut world, guard);

    let reports = boundary(
        &mut world,
        vec![WorldCommand::Despawn { entity: gone }],
        vec![
            turn(gone, &[("health", ScriptValue::Int(40))], 0),
            turn(kept, &[("health", ScriptValue::Int(30))], 0),
        ],
    );

    assert!(!world.contains(gone));
    assert_eq!(
        read(&world, kept, guard, "health"),
        Some(ScriptValue::Int(30))
    );
    assert!(reports.is_empty(), "{reports:?}");
}
