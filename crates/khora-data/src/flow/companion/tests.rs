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

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, InstanceLifecycle, PendingBody, PendingSequence,
    RecordedFault, ScriptSnapshot, ScriptValue, TimerRemaining,
};

use super::{companion_row, companion_snapshot};
use crate::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform, World,
};

/// The companion's fields, in order — the same for every behavior.
const COMPANION: [&str; 7] = [
    "state",
    "state_fields",
    "timers",
    "pending",
    "spawned",
    "fault",
    "resume_failed",
];

fn entity(index: u32, generation: u32) -> EntityId {
    EntityId { index, generation }
}

/// A guard part-way through everything a companion records: in `Alert`
/// with its data, two countdowns (one spent), an attack winding up whose
/// machine holds an entity, spawned, a fault, a body owed its
/// `OnResumeFailed`.
fn busy() -> ScriptSnapshot {
    ScriptSnapshot {
        fields: Vec::new(),
        state: Some("Alert".to_owned()),
        state_fields: vec![
            ("level".to_owned(), ScriptValue::Int(5)),
            ("since".to_owned(), ScriptValue::Float(0.25)),
            (
                "home".to_owned(),
                ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
            ),
        ],
        timers: vec![
            TimerRemaining {
                repeating: true,
                interval: 0.25,
                state: None,
                ordinal: 0,
                remaining: Some(0.1),
            },
            TimerRemaining {
                repeating: false,
                interval: 2.0,
                state: Some("Alert".to_owned()),
                ordinal: 0,
                remaining: None,
            },
        ],
        pending: Some(PendingSequence {
            fingerprint: 0xFEDC_BA98_7654_3210,
            remaining: 0.75,
            machine: FrozenMachine {
                body: PendingBody::Sequence,
                registers: vec![
                    FrozenValue::Int(1),
                    FrozenValue::Entity(entity(7, 3)),
                    FrozenValue::Text("winding".to_owned()),
                ],
                frames: vec![FrozenFrame {
                    function: "Guard::Attack".to_owned(),
                    base: 0,
                    return_pc: 0,
                    result: 0,
                    site: "expr.7d793037:await".to_owned(),
                    fingerprint: 0x0123_4567_89AB_CDEF,
                    locals: Vec::new(),
                    temporaries: Vec::new(),
                }],
                program_counter: 4,
                arguments: vec![FrozenValue::Int(7)],
            },
        }),
        lifecycle: InstanceLifecycle {
            spawned: true,
            fault: Some(RecordedFault {
                fingerprint: 0xDEAD_BEEF_0123_4567,
                reason: "a payload `Damaged` cannot take".to_owned(),
            }),
            resume_failed: vec!["Patrol.OnHit".to_owned()],
        },
    }
}

fn value_of<'r>(row: &'r [(String, ScriptValue)], name: &str) -> &'r ScriptValue {
    row.iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("the companion row has `{name}`: {row:?}"))
}

/// **A companion row is the companion's fields, in its layout's order** —
/// whatever the snapshot holds.
#[test]
fn a_companion_row_names_the_companion_fields_in_order() {
    for snapshot in [ScriptSnapshot::default(), busy()] {
        let row = companion_row(&snapshot).expect("a row");
        assert_eq!(
            row.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            COMPANION
        );
    }
}

/// **The companion holds the state by name** — never a number — and `null`
/// for a behavior in no state.
#[test]
fn the_companion_holds_the_state_by_name() {
    let row = companion_row(&busy()).expect("a row");
    assert_eq!(
        value_of(&row, "state"),
        &ScriptValue::Str("Alert".to_owned())
    );

    let row = companion_row(&ScriptSnapshot::default()).expect("a row");
    assert_eq!(value_of(&row, "state"), &ScriptValue::Null);
    assert_eq!(
        value_of(&row, "pending"),
        &ScriptValue::Null,
        "nothing waits"
    );
    assert_eq!(value_of(&row, "fault"), &ScriptValue::Null, "no fault");
}

/// **The lifecycle's flag is the companion's `bool`**: whether `OnSpawn`
/// ran.
#[test]
fn the_lifecycle_flag_is_the_companion_bool() {
    let row = companion_row(&busy()).expect("a row");
    assert_eq!(value_of(&row, "spawned"), &ScriptValue::Bool(true));

    let row = companion_row(&ScriptSnapshot::default()).expect("a row");
    assert_eq!(value_of(&row, "spawned"), &ScriptValue::Bool(false));
}

/// **A companion row round trips a snapshot**: state and its data, the
/// countdowns, the suspended sequence, the fault and the owed bodies.
#[test]
fn a_companion_row_round_trips_a_snapshot() {
    for snapshot in [busy(), ScriptSnapshot::default()] {
        let row = companion_row(&snapshot).expect("a row");
        let back = companion_snapshot(&row).expect("reads back");
        assert_eq!(back, snapshot);
    }
}

/// **The behavior's own fields are not the companion's**: they are its
/// component's, so a snapshot's fields neither reach the row nor come back
/// from it.
#[test]
fn a_companion_row_keeps_none_of_the_behavior_fields() {
    let snapshot = busy().with_field("hp", ScriptValue::Int(40));

    let row = companion_row(&snapshot).expect("a row");
    assert!(
        row.iter()
            .all(|(name, _)| COMPANION.contains(&name.as_str())),
        "{row:?}"
    );
    let back = companion_snapshot(&row).expect("reads back");
    assert!(back.fields.is_empty(), "{:?}", back.fields);
    assert_eq!(back.state, snapshot.state);
    assert_eq!(back.pending, snapshot.pending);
}

/// **The entity a suspended machine holds is an `Entity` value of the row**,
/// so a save remaps it like any reference a declared component holds.
#[test]
fn an_entity_in_a_pending_sequence_is_an_entity_value_of_the_row() {
    fn holds(value: &ScriptValue, wanted: EntityId) -> bool {
        match value {
            ScriptValue::Entity(id) => *id == wanted,
            ScriptValue::Array(items) => items.iter().any(|item| holds(item, wanted)),
            ScriptValue::Struct(fields) => fields.iter().any(|(_, item)| holds(item, wanted)),
            _ => false,
        }
    }
    let row = companion_row(&busy()).expect("a row");
    assert!(
        holds(value_of(&row, "pending"), entity(7, 3)),
        "{:?}",
        value_of(&row, "pending")
    );
}

/// **A row a companion cannot hold is refused**, never read as an empty
/// state: a `pending` that is no suspended sequence.
#[test]
fn a_companion_row_of_another_shape_is_refused() {
    let mut row = companion_row(&busy()).expect("a row");
    for (name, value) in &mut row {
        if name == "pending" {
            *value = ScriptValue::Int(3);
        }
    }
    assert!(companion_snapshot(&row).is_err());
}

/// **A companion row is a row the companion's columns hold**: attached to an
/// entity and read back from the `World`, it records the same snapshot.
#[test]
fn a_companion_row_is_stored_and_read_back_by_the_world() {
    let field = |name: &str, kind: FieldKind, default: ScriptValue| PackedField {
        name: name.to_owned(),
        kind,
        default,
    };
    let mut world = World::new();
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Guard#state".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Runtime,
            resumable: true,
            layout: PackedLayout::new(vec![
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
            ])
            .expect("a valid layout"),
        })
        .expect("the companion registers");
    let guard = world.spawn(Transform::identity());
    let row = companion_row(&busy()).expect("a row");
    world
        .add_runtime_component(guard, key, &ScriptValue::Struct(row.clone()))
        .expect("the companion holds its row");

    let stored = world
        .row(guard, key)
        .expect("the guard holds its companion");
    let read: Vec<(String, ScriptValue)> = COMPANION
        .iter()
        .enumerate()
        .map(|(slot, name)| {
            (
                (*name).to_owned(),
                stored.field(slot).expect("every field reads"),
            )
        })
        .collect();
    assert_eq!(read, row);
    assert_eq!(companion_snapshot(&read).expect("reads back"), busy());
}

/// **A companion holds no `initialised` flag**: an instance's defaults are
/// its layout's data, seeded the moment it exists, so nothing records
/// whether an initialiser ran — and a companion row without one reads back.
#[test]
fn a_companion_row_without_an_initialised_flag_reads_back() {
    let row = vec![
        ("state".to_owned(), ScriptValue::Null),
        ("state_fields".to_owned(), ScriptValue::Struct(Vec::new())),
        ("timers".to_owned(), ScriptValue::Array(Vec::new())),
        ("pending".to_owned(), ScriptValue::Null),
        ("spawned".to_owned(), ScriptValue::Bool(true)),
        ("fault".to_owned(), ScriptValue::Null),
        ("resume_failed".to_owned(), ScriptValue::Array(Vec::new())),
    ];
    let read = companion_snapshot(&row);
    assert!(
        read.is_ok(),
        "a companion of seven fields reads back: {:?}",
        read.err()
    );
}

/// **A companion row written before defaults were data still reads back**:
/// its `initialised` flag is a field the companion no longer declares, and
/// is ignored — whichever way it was set, the snapshot is the same.
#[test]
fn a_companion_row_still_holding_an_initialised_flag_reads_back() {
    let without = vec![
        ("state".to_owned(), ScriptValue::Null),
        ("state_fields".to_owned(), ScriptValue::Struct(Vec::new())),
        ("timers".to_owned(), ScriptValue::Array(Vec::new())),
        ("pending".to_owned(), ScriptValue::Null),
        ("spawned".to_owned(), ScriptValue::Bool(true)),
        ("fault".to_owned(), ScriptValue::Null),
        ("resume_failed".to_owned(), ScriptValue::Array(Vec::new())),
    ];
    let expected = companion_snapshot(&without).expect("the current row reads back");
    for flag in [false, true] {
        let mut older = without.clone();
        older.insert(4, ("initialised".to_owned(), ScriptValue::Bool(flag)));
        assert_eq!(
            companion_snapshot(&older).expect("an older row reads back"),
            expected,
            "`initialised: {flag}`"
        );
    }
}
