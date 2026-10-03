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

//! The self-describing form of a record: what JSON and MessagePack hold.

use std::collections::{BTreeMap, BTreeSet};

use khora_core::asset::AssetUUID;
use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::physics::{BodyType, ColliderShape};
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    ScriptValue, SuspendedMachine, TimerRemaining,
};
use khora_core::ui::types::{UiRect, UiVal};
use serde_json::json;

use crate::ecs::{
    AudioListener, Camera, Collider, Light, Name, Parent, ProjectionType, RigidBody, Script,
    SerializableAudioListener, SerializableCamera, SerializableCollider, SerializableLight,
    SerializableName, SerializableParent, SerializableRigidBody, SerializableScript,
    SerializableTag, SerializableTransform, Tag, Transform,
};
use crate::ui::{
    SerializableUiImage, SerializableUiStyle, SerializableUiText, UiImage, UiStyle, UiText,
};

use super::*;

/// The JSON a record is written as.
fn json_of(record: &Record) -> serde_json::Value {
    serde_json::to_value(record).unwrap_or_else(|e| panic!("{record:?} does not write: {e}"))
}

/// The record JSON text reads back as.
fn record_of(text: &str) -> Record {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("`{text}` does not read: {e}"))
}

/// Writes `value` as a record, the record as JSON text, and reads both back.
fn through_json<T: Serialize + DeserializeOwned>(value: &T) -> T {
    let name = std::any::type_name::<T>();
    let mut table = IdTable::default();
    let record = to_record(value, &mut table).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = serde_json::to_string(&record).unwrap_or_else(|e| panic!("{name} to JSON: {e}"));
    let back: Record =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} from JSON `{text}`: {e}"));
    from_record(&back, &mut table)
        .unwrap_or_else(|e| panic!("{name} does not read back from `{text}`: {e}"))
}

/// Writes `value` as a record, the record as named MessagePack, and reads both
/// back.
fn through_msgpack<T: Serialize + DeserializeOwned>(value: &T) -> T {
    let name = std::any::type_name::<T>();
    let mut table = IdTable::default();
    let record = to_record(value, &mut table).unwrap_or_else(|e| panic!("{name}: {e}"));
    let bytes =
        rmp_serde::to_vec_named(&record).unwrap_or_else(|e| panic!("{name} to MessagePack: {e}"));
    let back: Record =
        rmp_serde::from_slice(&bytes).unwrap_or_else(|e| panic!("{name} from MessagePack: {e}"));
    from_record(&back, &mut table)
        .unwrap_or_else(|e| panic!("{name} does not read back from MessagePack: {e}"))
}

/// Asserts `value` survives both self-describing forms, compared through
/// `Debug` (the generated mirrors do not implement `PartialEq`).
fn assert_self_describes<T: Serialize + DeserializeOwned + Debug>(value: &T) {
    let name = std::any::type_name::<T>();
    let expected = format!("{value:?}");
    assert_eq!(
        format!("{:?}", through_json(value)),
        expected,
        "{name} changed through JSON"
    );
    assert_eq!(
        format!("{:?}", through_msgpack(value)),
        expected,
        "{name} changed through MessagePack"
    );
}

/// A struct is a map keyed by its field names, in the order they were
/// written — what makes a text save read like the value it holds.
#[test]
fn a_struct_is_a_map_keyed_by_field_name() {
    let record = structure(
        "Vec3",
        vec![
            ("x", Record::F32(1.0)),
            ("y", Record::F32(-2.0)),
            ("z", Record::F32(0.5)),
        ],
    );
    assert_eq!(json_of(&record), json!({ "x": 1.0, "y": -2.0, "z": 0.5 }));
    assert_eq!(
        serde_json::to_string(&record).expect("writes"),
        r#"{"x":1.0,"y":-2.0,"z":0.5}"#
    );
}

/// A variant is its name when it carries nothing, and a map keyed by its name
/// when it does — serde's external tagging, which every JSON reader knows.
#[test]
fn a_variant_is_its_name_or_a_map_keyed_by_it() {
    assert_eq!(
        json_of(&variant("Mode", "Idle", VariantPayload::Unit)),
        json!("Idle")
    );
    assert_eq!(
        json_of(&variant(
            "Shape",
            "Sphere",
            VariantPayload::Newtype(Box::new(Record::F32(0.5)))
        )),
        json!({ "Sphere": 0.5 })
    );
    assert_eq!(
        json_of(&variant(
            "Shape",
            "Capsule",
            VariantPayload::Tuple(vec![Record::F32(1.0), Record::F32(0.25)])
        )),
        json!({ "Capsule": [1.0, 0.25] })
    );
    assert_eq!(
        json_of(&variant(
            "Light",
            "Point",
            struct_payload(vec![("range", Record::F32(12.0))])
        )),
        json!({ "Point": { "range": 12.0 } })
    );
}

/// An absent optional is `null`; a present one is its bare value, with no
/// wrapper a person would have to read through.
#[test]
fn an_option_is_null_or_its_bare_value() {
    assert_eq!(json_of(&Record::None), json!(null));
    assert_eq!(json_of(&Record::Some(Box::new(text("x")))), json!("x"));
    assert_eq!(json_of(&Record::Some(Box::new(Record::I64(-4)))), json!(-4));
}

/// The two reference kinds carry a marker key no field is named, and reading
/// the text back recovers them for what they are — an entity, an entity
/// outside the save, an asset — not as a map that happens to look like one.
#[test]
fn references_keep_their_markers() {
    let texture = AssetUUID::new_v5("textures/wall.png");
    let uuid_text = serde_json::to_value(texture).expect("a uuid is JSON");
    let created = PersistentId::created(3);

    let cases = [
        (
            Record::Entity(EntityRef::Id(PersistentId::authored(7))),
            json!({ "$entity": 7 }),
        ),
        (
            Record::Entity(EntityRef::Id(created)),
            json!({ "$entity": created.to_bits() }),
        ),
        (
            Record::Entity(EntityRef::Outside),
            json!({ "$entity": null }),
        ),
        (Record::Asset(texture), json!({ "$asset": uuid_text })),
    ];
    for (record, expected) in cases {
        let written = json_of(&record);
        assert_eq!(written, expected, "{record:?}");
        assert_eq!(
            record_of(&written.to_string()),
            record,
            "{written} did not read back as the reference it marks"
        );
    }
}

/// A marker key holding something that is not a reference is a damaged file,
/// not a map to be read as data.
#[test]
fn a_marker_with_the_wrong_payload_is_an_error() {
    for text in [
        r#"{"$entity": "seven"}"#,
        r#"{"$entity": -1}"#,
        r#"{"$asset": 5}"#,
        r#"{"$asset": "not a uuid"}"#,
    ] {
        assert!(
            serde_json::from_str::<Record>(text).is_err(),
            "`{text}` must be refused"
        );
    }
}

/// JSON prints an `f32` as the shortest decimal that names it — `0.1`, not
/// `0.10000000149011612` — and reading that decimal back into an `f32` must
/// give the very same bits.
#[test]
fn an_f32_keeps_its_shortest_decimal() {
    assert_eq!(
        serde_json::to_string(&Record::F32(0.1)).expect("writes"),
        "0.1"
    );
    for value in [
        0.1_f32,
        1.0 / 3.0,
        -7.25,
        16_777_216.0,
        f32::MAX,
        f32::MIN_POSITIVE,
        1.0e-45,
        123_456.79,
    ] {
        let text = serde_json::to_string(&Record::F32(value)).expect("writes");
        let back: f32 = read(&record_of(&text))
            .unwrap_or_else(|e| panic!("`{text}` does not read as an f32: {e}"));
        assert_eq!(
            back.to_bits(),
            value.to_bits(),
            "{value:?} came back as {back:?} through `{text}`"
        );
    }
}

/// A JSON number with a fraction is decimal text, not a double: it reads as a
/// decimal record, which reads into an `f32` as the `f32` the text names and
/// into an `f64` as the `f64` — refused only when it fits no `f32` at all. A
/// double a binary form holds stays a double, and the double nearest 0.1 is
/// no `f32`.
#[test]
fn a_json_fraction_is_a_decimal_read_at_the_field_width() {
    assert_eq!(record_of("0.1"), Record::Decimal(0.1));

    let single: f32 = read(&Record::Decimal(0.1)).expect("the decimal 0.1 is an f32");
    assert_eq!(single.to_bits(), 0.1_f32.to_bits());
    let double: f64 = read(&Record::Decimal(0.1)).expect("the decimal 0.1 is an f64");
    assert_eq!(double.to_bits(), 0.1_f64.to_bits());
    assert!(
        read::<f32>(&Record::Decimal(1.0e40)).is_err(),
        "a decimal beyond the f32 range is refused"
    );

    let bytes = rmp_serde::to_vec(&Record::F64(0.1)).expect("writes");
    let back: Record = rmp_serde::from_slice(&bytes).expect("reads");
    assert_eq!(back, Record::F64(0.1), "a binary double stays a double");
    assert!(
        read::<f32>(&Record::F64(0.1)).is_err(),
        "the double 0.1 is not narrowed into an f32"
    );
}

/// JSON keys are strings. A map keyed by integers is written with its keys as
/// text, and reading it back must parse them into integers again.
#[test]
fn a_string_map_key_reads_into_an_integer_key() {
    let by_slot: BTreeMap<u32, String> =
        BTreeMap::from([(3, "three".to_owned()), (10, "ten".to_owned())]);
    let back = through_json(&by_slot);
    assert_eq!(back, by_slot);
}

/// One busy behavior instance: state, timers, a pending sequence frozen with
/// an entity in a register.
fn busy_snapshot(target: EntityId) -> ScriptSnapshot {
    ScriptSnapshot {
        fields: vec![("target".into(), ScriptValue::Entity(target))],
        state: Some("Patrol".into()),
        state_fields: vec![("waypoint".into(), ScriptValue::Int(2))],
        timers: vec![TimerRemaining {
            repeating: true,
            interval: 0.5,
            state: None,
            ordinal: 0,
            remaining: Some(0.25),
        }],
        pending: Some(PendingSequence {
            fingerprint: 0xfeed_beef_dead_c0de,
            remaining: 1.5,
            machine: SuspendedMachine::Frozen(FrozenMachine {
                body: PendingBody::Timer {
                    index: 2,
                    rearm: FrozenValue::Float(0.5),
                },
                registers: vec![
                    FrozenValue::Unit,
                    FrozenValue::Int(-7),
                    FrozenValue::Entity(target),
                    FrozenValue::Literal("hello".into()),
                    FrozenValue::Vec3(Vec3::new(-1.0, 0.0, 1.0)),
                    FrozenValue::Null,
                ],
                frames: vec![FrozenFrame {
                    function: "on_hit".into(),
                    base: 0,
                    return_pc: 0,
                    result: 0,
                }],
                program_counter: 42,
            }),
        }),
    }
}

/// The text and MessagePack encodings write records in this form, so every
/// persisted type must come back through it exactly — structs from maps,
/// variants from their tags, newtypes and markers from what is left of them,
/// entities and assets from their markers.
#[test]
fn every_persisted_type_survives_the_self_describing_form() {
    let target = entity(5, 2);
    let texture = AssetUUID::new_v5("textures/wall.png");

    assert_self_describes(&SerializableTransform::from(Transform::new(
        Vec3::new(1.0, 2.0, 3.0),
        Quaternion::from_axis_angle(Vec3::Z, 0.3),
        Vec3::new(2.0, 2.0, 0.5),
    )));
    assert_self_describes(&SerializableName::from(Name::new("Hero")));
    assert_self_describes(&SerializableParent::from(Parent(target)));
    assert_self_describes(&SerializableTag::from(Tag(BTreeSet::from([
        "enemy".to_owned(),
        "boss".to_owned(),
    ]))));
    assert_self_describes(&SerializableAudioListener::from(AudioListener));
    assert_self_describes(&SerializableCamera::from(Camera::default_perspective()));
    assert_self_describes(&SerializableCamera::from(Camera::new_orthographic(
        640.0, 480.0, -1.0, 250.0,
    )));
    for projection in [
        ProjectionType::Perspective { fov_y_radians: 1.2 },
        ProjectionType::Orthographic {
            width: 16.0,
            height: 9.0,
        },
    ] {
        assert_self_describes(&projection);
    }
    for light_type in [
        LightType::Directional(DirectionalLight::default()),
        LightType::Point(PointLight {
            range: 12.0,
            color: LinearRgba::new(1.0, 0.8, 0.6, 1.0),
            ..PointLight::default()
        }),
        LightType::Spot(SpotLight::default()),
    ] {
        assert_self_describes(&SerializableLight::from(Light {
            light_type,
            enabled: false,
        }));
    }
    for shape in [
        ColliderShape::Box(Vec3::new(0.5, 1.0, 1.5)),
        ColliderShape::Sphere(0.75),
        ColliderShape::Capsule(1.0, 0.25),
    ] {
        assert_self_describes(&SerializableCollider::from(Collider {
            shape,
            friction: 0.9,
            ..Collider::default()
        }));
    }
    for body_type in [BodyType::Dynamic, BodyType::Static, BodyType::Kinematic] {
        assert_self_describes(&SerializableRigidBody::from(RigidBody {
            body_type,
            mass: 2.5,
            ..RigidBody::default()
        }));
    }
    assert_self_describes(&SerializableScript::from(Script {
        module: "ai/guard.erg".into(),
        behavior: "Guard".into(),
        fields: vec![
            ("speed".into(), ScriptValue::Float(0.1)),
            ("target".into(), ScriptValue::Entity(target)),
            ("unit".into(), ScriptValue::Unit),
            ("count".into(), ScriptValue::Int(i64::MIN)),
            ("aim".into(), ScriptValue::Vec2(Vec2::new(1.0, -2.0))),
            (
                "tint".into(),
                ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 0.4)),
            ),
            (
                "v4".into(),
                ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
            ),
            (
                "nested".into(),
                ScriptValue::Array(vec![
                    ScriptValue::Entity(target),
                    ScriptValue::Struct(vec![("inner".into(), ScriptValue::Bool(true))]),
                ]),
            ),
        ],
        runtime: busy_snapshot(target),
    }));
    assert_self_describes(&SerializableUiStyle::from(UiStyle {
        texture_id: Some(42),
        ..UiStyle::default()
    }));
    assert_self_describes(&SerializableUiStyle::from(UiStyle::default()));
    assert_self_describes(&SerializableUiImage::from(UiImage { texture }));
    assert_self_describes(&SerializableUiText::from(UiText {
        content: "Start".into(),
        font: texture,
        size: 18.0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
    }));
    assert_self_describes(&UiRect {
        left: UiVal::Px(1.0),
        right: UiVal::Percent(2.0),
        top: UiVal::Auto,
        bottom: UiVal::Px(4.0),
    });
}
