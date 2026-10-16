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

use serde::{Deserialize, Serialize};

use super::{from_value, to_value};
use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use crate::script::{
    FrozenFrame, FrozenLocal, FrozenMachine, FrozenValue, InstanceLifecycle, PendingBody,
    PendingSequence, RecordedFault, ScriptSnapshot, ScriptValue, TimerRemaining,
};

/// `value` written as a script value and read back.
fn round_trip<T>(value: &T) -> T
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let written = to_value(value).expect("writes as a script value");
    from_value(&written).unwrap_or_else(|e| panic!("reads back from {written:?}: {e}"))
}

fn entity(index: u32, generation: u32) -> EntityId {
    EntityId { index, generation }
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Patrol {
    name: String,
    stops: Vec<Point>,
    target: Option<EntityId>,
    visits: u32,
    alert: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Order {
    Hold,
    Wait(f32),
    Goto(f32, f32),
    Follow { leader: EntityId, distance: f32 },
}

// ─── Shapes ─────────────────────────────────────────────────────────────────

/// **Scalars are the script values of their kind**: an integer an `Int`, a
/// float a `Float`, text a `Str`, a flag a `Bool`.
#[test]
fn a_scalar_is_the_script_value_of_its_kind() {
    assert_eq!(to_value(&7_i64).expect("writes"), ScriptValue::Int(7));
    assert_eq!(to_value(&-3_i32).expect("writes"), ScriptValue::Int(-3));
    assert_eq!(to_value(&12_u32).expect("writes"), ScriptValue::Int(12));
    assert_eq!(to_value(&0.5_f32).expect("writes"), ScriptValue::Float(0.5));
    assert_eq!(to_value(&true).expect("writes"), ScriptValue::Bool(true));
    assert_eq!(
        to_value("wind-up").expect("writes"),
        ScriptValue::Str("wind-up".to_owned())
    );
}

/// **A struct is a `Struct` of its fields by name, in declaration order.**
#[test]
fn a_struct_is_its_fields_by_name() {
    assert_eq!(
        to_value(&Point { x: 1.0, y: -2.0 }).expect("writes"),
        fields(&[
            ("x", ScriptValue::Float(1.0)),
            ("y", ScriptValue::Float(-2.0))
        ])
    );
}

/// **A sequence is an `Array`**, element by element.
#[test]
fn a_sequence_is_an_array() {
    assert_eq!(
        to_value(&vec![1_i64, 2, 3]).expect("writes"),
        ScriptValue::Array(vec![
            ScriptValue::Int(1),
            ScriptValue::Int(2),
            ScriptValue::Int(3)
        ])
    );
}

/// **An absent `Option` is `Null`; a present one is its value**, not wrapped.
#[test]
fn an_option_is_null_or_its_value() {
    assert_eq!(to_value(&None::<i64>).expect("writes"), ScriptValue::Null);
    assert_eq!(to_value(&Some(4_i64)).expect("writes"), ScriptValue::Int(4));
}

/// **An entity is an `Entity`** — not a struct of two numbers — wherever it
/// sits, so the references a save remaps are found: alone, in a list, in an
/// option, in a struct.
#[test]
fn an_entity_is_an_entity_value_wherever_it_sits() {
    let guard = entity(7, 3);
    assert_eq!(
        to_value(&guard).expect("writes"),
        ScriptValue::Entity(guard)
    );
    assert_eq!(
        to_value(&vec![guard]).expect("writes"),
        ScriptValue::Array(vec![ScriptValue::Entity(guard)])
    );
    assert_eq!(
        to_value(&Some(guard)).expect("writes"),
        ScriptValue::Entity(guard)
    );
    let patrol = Patrol {
        name: "east".to_owned(),
        stops: Vec::new(),
        target: Some(guard),
        visits: 0,
        alert: false,
    };
    let written = to_value(&patrol).expect("writes");
    let target = written
        .as_fields()
        .and_then(|fields| fields.iter().find(|(name, _)| name == "target"))
        .map(|(_, value)| value.clone());
    assert_eq!(target, Some(ScriptValue::Entity(guard)));
}

/// **An enum variant with a payload is a one-field `Struct` named by the
/// variant.**
#[test]
fn an_enum_variant_is_a_struct_named_by_the_variant() {
    assert_eq!(
        to_value(&Order::Wait(2.0)).expect("writes"),
        fields(&[("Wait", ScriptValue::Float(2.0))])
    );
    let follow = to_value(&Order::Follow {
        leader: entity(1, 1),
        distance: 3.0,
    })
    .expect("writes");
    let (variant, payload) = match &follow {
        ScriptValue::Struct(fields) if fields.len() == 1 => fields[0].clone(),
        other => panic!("one field named by the variant, got {other:?}"),
    };
    assert_eq!(variant, "Follow");
    assert_eq!(
        payload,
        fields(&[
            ("leader", ScriptValue::Entity(entity(1, 1))),
            ("distance", ScriptValue::Float(3.0)),
        ])
    );
}

// ─── Round trips ────────────────────────────────────────────────────────────

/// **Every scalar comes back as it went in**, the integer bounds included —
/// a fingerprint is a `u64` past what an `int` holds.
#[test]
fn scalars_round_trip_at_their_bounds() {
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [0_u64, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        assert_eq!(round_trip(&value), value, "{value:#x}");
    }
    for value in [u32::MIN, u32::MAX] {
        assert_eq!(round_trip(&value), value);
    }
    for value in [0.0_f32, -0.5, f32::MAX, f32::MIN_POSITIVE] {
        assert_eq!(round_trip(&value), value);
    }
    assert!(round_trip(&true));
    assert_eq!(round_trip(&String::new()), String::new());
    assert_eq!(round_trip(&"é ✓".to_owned()), "é ✓".to_owned());
}

#[test]
fn a_struct_round_trips() {
    let point = Point { x: 1.5, y: -0.25 };
    assert_eq!(round_trip(&point), point);
}

#[test]
fn every_enum_variant_round_trips() {
    for order in [
        Order::Hold,
        Order::Wait(0.75),
        Order::Goto(1.0, -1.0),
        Order::Follow {
            leader: entity(9, 2),
            distance: 4.0,
        },
    ] {
        assert_eq!(round_trip(&order), order);
    }
}

#[test]
fn options_and_vectors_round_trip() {
    assert_eq!(round_trip(&None::<String>), None);
    assert_eq!(round_trip(&Some("x".to_owned())), Some("x".to_owned()));
    assert_eq!(round_trip(&Vec::<i64>::new()), Vec::<i64>::new());
    assert_eq!(
        round_trip(&vec![Some(1_i64), None, Some(3)]),
        vec![Some(1_i64), None, Some(3)]
    );
    assert_eq!(
        round_trip(&vec![vec![1_u32], vec![], vec![2, 3]]),
        vec![vec![1_u32], vec![], vec![2, 3]]
    );
}

/// **A nested value round trips**: a struct holding a list of structs, an
/// optional entity, a tuple, enums inside a list.
#[test]
fn a_nested_value_round_trips() {
    let patrol = Patrol {
        name: "east wall".to_owned(),
        stops: vec![Point { x: 0.0, y: 1.0 }, Point { x: 2.0, y: 3.0 }],
        target: Some(entity(4, 1)),
        visits: 3,
        alert: true,
    };
    assert_eq!(round_trip(&patrol), patrol);
    let plan = (
        patrol.clone(),
        vec![Order::Hold, Order::Wait(1.0)],
        Some(entity(0, 0)),
    );
    assert_eq!(round_trip(&plan), plan);
}

#[test]
fn an_entity_round_trips() {
    for id in [entity(0, 0), entity(7, 3), entity(u32::MAX, u32::MAX)] {
        assert_eq!(round_trip(&id), id);
        assert_eq!(from_value::<EntityId>(&ScriptValue::Entity(id)), Ok(id));
    }
}

/// **The engine's math types round trip** — a script value already holds
/// them, and a frozen register does.
#[test]
fn math_values_round_trip() {
    let v2 = Vec2::new(1.0, -2.0);
    let v3 = Vec3::new(1.0, 2.0, 3.0);
    let v4 = Vec4::new(1.0, 2.0, 3.0, 4.0);
    let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
    let c = LinearRgba::new(0.25, 0.5, 0.75, 1.0);
    assert_eq!(round_trip(&v2), v2);
    assert_eq!(round_trip(&v3), v3);
    assert_eq!(round_trip(&v4), v4);
    assert_eq!(round_trip(&q), q);
    assert_eq!(round_trip(&c), c);
}

/// **A script value itself round trips** — a state's data is a list of named
/// script values, each one any variant.
#[test]
fn every_script_value_round_trips() {
    let every = vec![
        ScriptValue::Unit,
        ScriptValue::Bool(true),
        ScriptValue::Int(-9),
        ScriptValue::Float(0.5),
        ScriptValue::Str("lit".to_owned()),
        ScriptValue::Vec2(Vec2::new(1.0, 2.0)),
        ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
        ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ScriptValue::Quat(Quaternion::IDENTITY),
        ScriptValue::Color(LinearRgba::WHITE),
        ScriptValue::Entity(entity(5, 2)),
        ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Null]),
        fields(&[("hp", ScriptValue::Int(3)), ("name", ScriptValue::Null)]),
        ScriptValue::Null,
    ];
    for value in &every {
        assert_eq!(&round_trip(value), value);
    }
    let named: Vec<(String, ScriptValue)> = every
        .iter()
        .enumerate()
        .map(|(i, value)| (format!("f{i}"), value.clone()))
        .collect();
    assert_eq!(round_trip(&named), named);
}

// ─── What a companion holds ─────────────────────────────────────────────────

/// A machine two calls deep, holding one register of every kind, entities
/// among them.
fn frozen(body: PendingBody) -> FrozenMachine {
    FrozenMachine {
        body,
        registers: vec![
            FrozenValue::Unit,
            FrozenValue::Int(-42),
            FrozenValue::Float(0.5),
            FrozenValue::Bool(true),
            FrozenValue::Entity(entity(7, 3)),
            FrozenValue::Literal("wind-up".to_owned()),
            FrozenValue::Expired,
            FrozenValue::Vec2(Vec2::new(1.0, -2.0)),
            FrozenValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
            FrozenValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
            FrozenValue::Quat(Quaternion::new(0.0, 0.0, 0.0, 1.0)),
            FrozenValue::Color(LinearRgba::new(0.25, 0.5, 0.75, 1.0)),
            FrozenValue::Null,
            FrozenValue::Text("built".to_owned()),
            FrozenValue::Array(vec![FrozenValue::Int(1), FrozenValue::Entity(entity(2, 1))]),
            FrozenValue::Struct {
                name: "Point".to_owned(),
                fields: vec![("x".to_owned(), FrozenValue::Float(1.0))],
            },
        ],
        frames: vec![
            FrozenFrame {
                function: "Guard::OnSpotted".to_owned(),
                base: 0,
                return_pc: 0,
                result: 0,
                site: "expr.5d41402a:call.Guard::Attack".to_owned(),
                fingerprint: 0x0123_4567_89AB_CDEF,
                locals: vec![FrozenLocal {
                    name: "by".to_owned(),
                    ty: "int".to_owned(),
                    scope: "body".to_owned(),
                    register: 1,
                }],
                temporaries: vec![2, 3],
            },
            FrozenFrame {
                function: "Guard::Attack".to_owned(),
                base: 6,
                return_pc: 3,
                result: 2,
                site: "expr.7d793037:await".to_owned(),
                fingerprint: 0xFEDC_BA98_7654_3210,
                locals: Vec::new(),
                temporaries: Vec::new(),
            },
        ],
        program_counter: 4,
        arguments: vec![FrozenValue::Int(7)],
    }
}

fn every_body() -> Vec<PendingBody> {
    vec![
        PendingBody::Sequence,
        PendingBody::Spawn,
        PendingBody::Update,
        PendingBody::Timer {
            timer: "Guard.__every(0.5)".to_owned(),
            rearm: FrozenValue::Float(0.5),
        },
        PendingBody::Timer {
            timer: "Guard.Patrol.__after(2)".to_owned(),
            rearm: FrozenValue::Null,
        },
    ]
}

/// **A suspended sequence round trips**, its machine whole, whatever body it
/// is part-way through — the fingerprints past an `int`'s range included.
#[test]
fn a_pending_sequence_with_a_frozen_machine_round_trips() {
    for body in every_body() {
        let pending = PendingSequence {
            fingerprint: 0xDEAD_BEEF_0123_4567,
            remaining: 0.75,
            running: 0.0,
            machine: frozen(body.clone()),
        };
        assert_eq!(round_trip(&pending), pending, "{body:?}");
    }
}

/// **The entities a frozen machine holds are `Entity` values** in what it is
/// written as, so a save remaps them like any reference.
#[test]
fn the_entities_of_a_frozen_machine_are_entity_values() {
    fn entities(value: &ScriptValue, found: &mut Vec<EntityId>) {
        match value {
            ScriptValue::Entity(id) => found.push(*id),
            ScriptValue::Array(items) => items.iter().for_each(|item| entities(item, found)),
            ScriptValue::Struct(fields) => {
                fields.iter().for_each(|(_, item)| entities(item, found))
            }
            _ => {}
        }
    }
    let written = to_value(&frozen(PendingBody::Sequence)).expect("writes");
    let mut found = Vec::new();
    entities(&written, &mut found);
    found.sort_by_key(|id| (id.index, id.generation));
    assert_eq!(found, vec![entity(2, 1), entity(7, 3)], "{written:?}");
}

/// **A whole snapshot round trips**: fields, state and its data, countdowns
/// (a spent one too), a suspended sequence and the lifecycle with its fault.
#[test]
fn a_script_snapshot_round_trips() {
    let snapshot = ScriptSnapshot {
        fields: vec![
            ("hp".to_owned(), ScriptValue::Int(40)),
            ("target".to_owned(), ScriptValue::Entity(entity(3, 1))),
        ],
        state: Some("Alert".to_owned()),
        state_fields: vec![
            ("level".to_owned(), ScriptValue::Int(5)),
            ("seen".to_owned(), ScriptValue::Null),
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
                ordinal: 1,
                remaining: None,
            },
        ],
        pending: Some(PendingSequence {
            fingerprint: u64::MAX,
            remaining: 0.5,
            running: 0.0,
            machine: frozen(PendingBody::Timer {
                timer: "Guard.__every(0.25)#1".to_owned(),
                rearm: FrozenValue::Float(0.25),
            }),
        }),
        lifecycle: InstanceLifecycle {
            owed: 0.0,
            spawned: true,
            fault: Some(RecordedFault {
                fingerprint: 0x8000_0000_0000_0001,
                reason: "division by zero".to_owned(),
            }),
            resume_failed: vec!["OnSpawn".to_owned(), "Patrol.OnHit".to_owned()],
        },
    };
    assert_eq!(round_trip(&snapshot), snapshot);
    assert_eq!(
        round_trip(&ScriptSnapshot::default()),
        ScriptSnapshot::default()
    );
}

// ─── Refusals ───────────────────────────────────────────────────────────────

/// **A value of another shape is refused**, never guessed at: text for an
/// integer, a struct missing a field, a variant no enum has.
#[test]
fn a_value_of_another_shape_is_refused() {
    assert!(from_value::<i64>(&ScriptValue::Str("7".to_owned())).is_err());
    assert!(from_value::<Point>(&fields(&[("x", ScriptValue::Float(1.0))])).is_err());
    assert!(from_value::<Order>(&fields(&[("Run", ScriptValue::Unit)])).is_err());
    assert!(from_value::<EntityId>(&ScriptValue::Int(3)).is_err());
}
