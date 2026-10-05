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

//! The snapshot encoding at its edges: the schema tracer, the positional codec.

#![allow(dead_code)]

use khora_core::asset::AssetUUID;
use khora_data::scene::positional::{from_positional, to_positional};
use khora_data::scene::record::{EntityRef, RecordError, ReferenceReader, ReferenceWriter};
use khora_data::scene::schema::schema_of;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::renames::SerializableStamina;
use super::*;

struct NoRefs;

impl ReferenceWriter for NoRefs {
    fn write_entity(&mut self, _entity: EntityId) -> EntityRef {
        EntityRef::Outside
    }
}

impl ReferenceReader for NoRefs {
    fn read_entity(&mut self, _reference: EntityRef) -> Result<EntityId, RecordError> {
        Err(RecordError("no entities here".into()))
    }
}

fn fingerprint<T: DeserializeOwned>() -> u64 {
    schema_of::<T>().fingerprint()
}

fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> Result<T, RecordError> {
    let mut bytes = Vec::new();
    to_positional(value, &mut bytes, &mut NoRefs).expect("the value is written");
    from_positional::<T>(&bytes, &mut NoRefs)
}

// --- A field after an asset reference -------------------------------------

mod asset_then_f32 {
    use super::*;
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    pub struct Probe {
        pub texture: AssetUUID,
        pub a: f32,
        pub b: f32,
    }
}

mod asset_then_f64 {
    use super::*;
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    pub struct Probe {
        pub texture: AssetUUID,
        pub c: f64,
    }
}

/// The tracer answers `bytes` with an empty slice; `AssetUUID` (through
/// `Uuid`) refuses anything but 16 bytes, so the run stops there and every
/// field after the asset reference is never traced — the container is left
/// out and the struct is fingerprinted by its name alone.
#[test]
fn a_field_after_an_asset_reference_is_in_the_fingerprint() {
    let text = schema_of::<asset_then_f32::Probe>().to_string();
    assert!(
        text.contains("a:f32"),
        "the fields after the asset reference are not traced: `{text}`"
    );
    assert_ne!(
        fingerprint::<asset_then_f32::Probe>(),
        fingerprint::<asset_then_f64::Probe>(),
        "two f32 retyped as one f64 after an asset reference: same fingerprint"
    );
}

/// A registered component hit by it: `UiText { content, font, size, color }`
/// is fingerprinted as `content` and its name — `size` and `color` can change
/// type and a snapshot still loads.
#[test]
fn a_registered_ui_text_fingerprints_its_fields() {
    let text = schema_of::<khora_data::ui::SerializableUiText>().to_string();
    assert!(
        text.contains("size:f32"),
        "UiText's fields after `font` are not in its schema: `{text}`"
    );
}

/// The consequence: a value written by the first schema is read by the
/// second with no error — 8 bytes of two f32 silently become one f64 — and
/// the fingerprints that should have refused it are equal.
#[test]
fn a_value_of_another_schema_is_not_read_silently() {
    let mut bytes = Vec::new();
    to_positional(
        &asset_then_f32::Probe {
            texture: AssetUUID::new_v5("t.png"),
            a: 1.0,
            b: 2.0,
        },
        &mut bytes,
        &mut NoRefs,
    )
    .expect("written");
    let misread = from_positional::<asset_then_f64::Probe>(&bytes, &mut NoRefs);
    let same_fingerprint =
        fingerprint::<asset_then_f32::Probe>() == fingerprint::<asset_then_f64::Probe>();
    assert!(
        misread.is_err() || !same_fingerprint,
        "read by another schema with an equal fingerprint: {misread:?}"
    );
}

// --- Two types under one serde name ---------------------------------------

mod first_u32 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Inner {
        pub x: u32,
    }
}
mod first_u64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Inner {
        pub x: u64,
    }
}
mod second {
    use super::*;
    #[derive(Deserialize)]
    pub struct Inner {
        pub label: String,
    }
}
mod pair_before {
    use super::*;
    #[derive(Deserialize)]
    #[serde(rename = "Pair")]
    pub struct Pair {
        pub a: first_u32::Inner,
        pub b: second::Inner,
    }
}
mod pair_after {
    use super::*;
    #[derive(Deserialize)]
    #[serde(rename = "Pair")]
    pub struct Pair {
        pub a: first_u64::Inner,
        pub b: second::Inner,
    }
}

/// Containers are keyed by their serde name alone: a second `Inner` from
/// another module overwrites the first, and a change to the first no longer
/// reaches the fingerprint.
#[test]
fn two_types_sharing_a_serde_name_are_both_fingerprinted() {
    assert_ne!(
        fingerprint::<pair_before::Pair>(),
        fingerprint::<pair_after::Pair>(),
        "`{}` vs `{}`",
        schema_of::<pair_before::Pair>(),
        schema_of::<pair_after::Pair>()
    );
}

// --- Recursion through a map's value ---------------------------------------

mod node_f32 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub children: std::collections::BTreeMap<String, Node>,
        pub weight: f32,
    }
}
mod node_f64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub children: std::collections::BTreeMap<String, Node>,
        pub weight: f64,
    }
}

/// A sequence whose element recurses ends empty; a map whose *value*
/// recurses aborts the run, so the fields after it are never traced.
#[test]
fn a_type_recursing_through_a_map_value_fingerprints_its_later_fields() {
    assert_ne!(
        fingerprint::<node_f32::Node>(),
        fingerprint::<node_f64::Node>(),
        "`{}`",
        schema_of::<node_f32::Node>()
    );
}

// --- Writer and reader count depth differently -----------------------------

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Chain {
    End,
    Link(u8, Box<Chain>),
}

fn chain(links: usize) -> Chain {
    (0..links).fold(Chain::End, |next, _| Chain::Link(7, Box::new(next)))
}

/// The writer takes one level for a tuple variant; the reader takes two (the
/// enum, then its elements). A value the writer accepts as within
/// `MAX_DEPTH` (128) is refused by the reader.
#[test]
fn what_the_positional_writer_accepts_the_reader_reads() {
    let value = chain(100);
    assert_eq!(
        round_trip(&value).as_ref().ok(),
        Some(&value),
        "{:?}",
        round_trip(&value).err()
    );
}

// --- Elements that take no bytes -----------------------------------------

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
struct Marker;

/// A count is refused when larger than the bytes left — but a unit element
/// takes no byte, so a sequence of them is written and cannot be read.
#[test]
fn a_sequence_of_unit_elements_reads_back() {
    let value = vec![Marker, Marker, Marker];
    assert_eq!(round_trip(&value), Ok(value));
}

// --- Registered materials ---------------------------------------------------

/// `StandardMaterial` holds `base_color_texture: Option<AssetUUID>` second:
/// the tracer stops there, so `metallic`, `roughness` … are not in the
/// fingerprint `MaterialRef`'s snapshot schema is built from.
#[test]
fn the_standard_material_schema_holds_every_field() {
    use khora_core::asset::StandardMaterial;
    let text = schema_of::<StandardMaterial>().to_string();
    for field in ["metallic:f32", "roughness:f32", "double_sided:bool"] {
        assert!(text.contains(field), "`{field}` is not in `{text}`");
    }
}

/// An inline material of every registered type comes back from a snapshot
/// exactly as a record brings it back.
#[test]
fn an_inline_material_of_every_type_round_trips_through_a_snapshot() {
    use khora_core::asset::{
        AlphaMode, EmissiveMaterial, Material, StandardMaterial, UnlitMaterial, WireframeMaterial,
    };
    use khora_core::math::LinearRgba;
    use khora_data::ecs::MaterialRef;
    use khora_data::scene::material_to_json;
    use khora_data::scene::snapshot::{prepare_snapshot, write_snapshot};

    let color = LinearRgba::new(0.1, 0.2, 0.3, 0.4);
    let texture = Some(AssetUUID::new_v5("t/a.png"));
    let materials: Vec<Box<dyn Material>> = vec![
        Box::new(StandardMaterial {
            base_color: color,
            base_color_texture: texture,
            metallic: 0.7,
            roughness: 0.2,
            metallic_roughness_texture: Some(AssetUUID::new_v5("t/mr.png")),
            normal_map: Some(AssetUUID::new_v5("t/n.png")),
            occlusion_map: None,
            emissive: LinearRgba::new(1.0, 0.5, 0.25, 1.0),
            emissive_texture: Some(AssetUUID::new_v5("t/e.png")),
            alpha_mode: AlphaMode::Blend,
            alpha_cutoff: 0.33,
            double_sided: true,
        }),
        Box::new(UnlitMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            alpha_cutoff: 0.6,
        }),
        Box::new(EmissiveMaterial {
            emissive_color: color,
            intensity: 4.5,
            alpha_mode: AlphaMode::Blend,
        }),
        Box::new(WireframeMaterial {
            color,
            line_width: 2.5,
        }),
    ];
    let mut src = World::new();
    let entities: Vec<EntityId> = materials
        .into_iter()
        .map(|material| src.spawn(MaterialRef::inline(material)))
        .collect();
    let file = write_snapshot(&src).expect("written");
    let mut dst = World::new();
    let applied = prepare_snapshot(&mut dst, &file)
        .unwrap_or_else(|e| panic!("refused: {e}"))
        .commit(&mut dst, Identity::Keep);
    for (entity, (_, twin)) in entities.iter().zip(&applied.entities) {
        let (
            Some(MaterialRef::Inline {
                material: a,
                uuid: ua,
            }),
            Some(MaterialRef::Inline {
                material: b,
                uuid: ub,
            }),
        ) = (
            src.get::<MaterialRef>(*entity),
            dst.get::<MaterialRef>(*twin),
        )
        else {
            panic!("not inline after the round trip");
        };
        assert_eq!(material_to_json(&**a), material_to_json(&**b));
        assert_eq!(ua, ub);
    }
}

// --- Determinism ------------------------------------------------------------

/// The same world snapshotted twice is the same bytes.
#[test]
fn the_same_world_snapshots_to_the_same_bytes() {
    use khora_data::scene::snapshot::write_snapshot;
    let (world, _) = super::sample::sample_world();
    let a = write_snapshot(&world).expect("written").to_bytes();
    let b = write_snapshot(&world).expect("written").to_bytes();
    assert_eq!(a, b);
}

// --- Adversarial payloads ---------------------------------------------------

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn file_with(payload: Vec<u8>) -> khora_core::scene::SceneFile {
    use khora_data::scene::snapshot::write_snapshot;
    let mut file = write_snapshot(&World::new()).expect("written");
    file.header.payload_length = payload.len() as u64;
    file.payload = payload;
    file
}

/// One component listed twice in the table, a page naming both entries: the
/// same component twice in a page, refused.
#[test]
fn one_component_through_two_table_entries_is_refused() {
    use khora_data::ecs::Name;
    use khora_data::scene::snapshot::prepare_snapshot;
    let mut src = World::new();
    let e = src.spawn(Name::new("x"));
    let id = src.persistent_id(e).expect("an id");
    let reg = saved_registrations()
        .find(|reg| reg.type_name == "Name")
        .expect("Name is registered");
    let mut payload = vec![1];
    varint(&mut payload, 2);
    for _ in 0..2 {
        varint(&mut payload, 4);
        payload.extend_from_slice(b"Name");
        payload.extend_from_slice(&(reg.schema)().to_le_bytes());
    }
    varint(&mut payload, 1);
    payload.extend_from_slice(&id.to_bits().to_le_bytes());
    varint(&mut payload, 1);
    varint(&mut payload, 2);
    varint(&mut payload, 0);
    varint(&mut payload, 1);
    varint(&mut payload, 1);
    payload.extend_from_slice(&id.to_bits().to_le_bytes());
    for _ in 0..2 {
        varint(&mut payload, 2);
        payload.extend_from_slice(&[1, b'a']);
    }
    let mut dst = World::new();
    let before = dst.iter_entities().count();
    let refused = prepare_snapshot(&mut dst, &file_with(payload))
        .err()
        .expect("refused");
    assert!(refused.message.contains("Name"), "{}", refused.message);
    assert_eq!(dst.iter_entities().count(), before);
}

// --- Frozen machines -------------------------------------------------------

fn machines() -> Vec<khora_core::script::FrozenMachine> {
    use khora_core::script::{FrozenFrame, FrozenMachine, FrozenValue, PendingBody};
    vec![
        FrozenMachine {
            body: PendingBody::Spawn,
            registers: vec![],
            frames: vec![],
            program_counter: 0,
            arguments: Vec::new(),
        },
        FrozenMachine {
            body: PendingBody::Sequence,
            registers: vec![
                FrozenValue::Int(-3),
                FrozenValue::Float(0.5),
                FrozenValue::Literal("é".into()),
            ],
            frames: vec![],
            program_counter: 9,
            arguments: Vec::new(),
        },
        FrozenMachine {
            body: PendingBody::Timer {
                timer: "Guard.__every(0.25)".to_owned(),
                rearm: FrozenValue::Float(0.25),
            },
            registers: vec![FrozenValue::Unit, FrozenValue::Expired, FrozenValue::Null],
            frames: vec![
                FrozenFrame {
                    function: "Guard::OnSpotted".into(),
                    base: 0,
                    return_pc: 0,
                    result: 0,
                    site: String::new(),
                    fingerprint: 0,
                    locals: Vec::new(),
                    temporaries: Vec::new(),
                },
                FrozenFrame {
                    function: "Guard::Attack".into(),
                    base: 1,
                    return_pc: 3,
                    result: 2,
                    site: String::new(),
                    fingerprint: 0,
                    locals: Vec::new(),
                    temporaries: Vec::new(),
                },
            ],
            program_counter: u64::MAX,
            arguments: Vec::new(),
        },
    ]
}

/// A frozen machine, held directly — no wrapper, no form to tell apart —
/// through a record and every self-describing carrier of one, through
/// serde_json and rmp directly, and by position.
#[test]
fn a_frozen_machine_round_trips_everywhere() {
    use khora_core::script::FrozenMachine;
    use khora_data::scene::record::{from_record, to_record, Record};
    struct Nothing;
    impl ReferenceWriter for Nothing {
        fn write_entity(&mut self, _e: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    for machine in machines() {
        let record = to_record(&machine, &mut Nothing).expect("record");
        assert_eq!(
            from_record::<FrozenMachine>(&record, &mut NoRefs).as_ref(),
            Ok(&machine)
        );
        let json: Record = serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(
            from_record::<FrozenMachine>(&json, &mut NoRefs).as_ref(),
            Ok(&machine),
            "json record"
        );
        let mp: Record = rmp_serde::from_slice(&rmp_serde::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            from_record::<FrozenMachine>(&mp, &mut NoRefs).as_ref(),
            Ok(&machine),
            "msgpack record"
        );
        let direct_json: FrozenMachine =
            serde_json::from_str(&serde_json::to_string(&machine).unwrap()).unwrap();
        assert_eq!(direct_json, machine, "direct json");
        let direct_mp: Result<FrozenMachine, _> =
            rmp_serde::from_slice(&rmp_serde::to_vec(&machine).unwrap());
        assert_eq!(direct_mp.ok().as_ref(), Some(&machine), "direct msgpack");
        assert_eq!(round_trip(&machine).as_ref(), Ok(&machine), "positional");
    }
}

/// A pending sequence holds its machine as a plain field: the sequence round
/// trips through a record and by position with the machine it holds intact.
#[test]
fn a_pending_sequence_round_trips_with_its_frozen_machine() {
    use khora_core::script::PendingSequence;
    use khora_data::scene::record::{from_record, to_record};
    struct Nothing;
    impl ReferenceWriter for Nothing {
        fn write_entity(&mut self, _e: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    for machine in machines() {
        let sequence = PendingSequence {
            fingerprint: 0xDEAD_BEEF_0123_4567,
            remaining: 0.75,
            machine,
        };
        let record = to_record(&sequence, &mut Nothing).expect("record");
        assert_eq!(
            from_record::<PendingSequence>(&record, &mut NoRefs).as_ref(),
            Ok(&sequence),
            "record"
        );
        assert_eq!(round_trip(&sequence).as_ref(), Ok(&sequence), "positional");
    }
}

/// A registration whose trace stopped at its root is fingerprinted by its
/// mirror's name alone: any change to its fields leaves a snapshot loadable.
#[test]
fn no_registration_is_fingerprinted_by_its_name_alone() {
    use khora_data::scene::schema::fingerprint_of;
    let blind: Vec<&str> = saved_registrations()
        .filter(|reg| {
            let fingerprint = (reg.schema)();
            fingerprint == fingerprint_of(&format!("Serializable{}", reg.type_name))
                || fingerprint == fingerprint_of(reg.type_name)
        })
        .map(|reg| reg.type_name)
        .collect();
    assert!(blind.is_empty(), "fingerprinted by name alone: {blind:?}");
}

mod alias_f32 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        #[serde(alias = "points")]
        pub reserve: f32,
        pub regen: f32,
    }
}
mod alias_f64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        #[serde(alias = "points")]
        pub reserve: f32,
        pub regen: f64,
    }
}

/// serde lists a field's aliases among the struct's fields: the tracer asks
/// for three elements, the type reads two, the container is dropped and the
/// struct is fingerprinted by its name — what `#[component(formerly = …)]`
/// on a field does to every renamed component (`Stamina` here).
#[test]
fn a_field_with_a_former_name_keeps_its_struct_in_the_fingerprint() {
    assert_ne!(
        fingerprint::<alias_f32::Probe>(),
        fingerprint::<alias_f64::Probe>(),
        "`{}`",
        schema_of::<alias_f32::Probe>()
    );
    // serde lists aliased fields' names flat, so names and types cannot be
    // paired: what matters is that the struct is traced whole — not by its
    // name alone, not marked incomplete — with every name and every type.
    let stamina = schema_of::<SerializableStamina>().to_string();
    assert_ne!(
        stamina, "SerializableStamina",
        "fingerprinted by name alone"
    );
    assert!(!stamina.contains("!incomplete"), "`{stamina}`");
    for name in ["reserve", "points", "regen"] {
        assert!(stamina.contains(name), "`{name}` is not in `{stamina}`");
    }
    assert_eq!(stamina.matches("f32").count(), 2, "`{stamina}`");
}

// --- A pending sequence holding every register kind, through a whole scene --

/// A behavior stopped in a schedule's body, its machine holding one register
/// of every kind — non-finite floats, empty and non-ASCII literals, an entity
/// — under a deep call stack.
fn every_register_snapshot(target: EntityId) -> khora_core::script::ScriptSnapshot {
    use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
    use khora_core::script::{
        FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    };
    let frames = (0..300u64)
        .map(|depth| FrozenFrame {
            function: format!("Guard::étape_{depth}_守"),
            base: depth,
            return_pc: depth * 3,
            result: depth,
            site: String::new(),
            fingerprint: 0,
            locals: Vec::new(),
            temporaries: Vec::new(),
        })
        .collect();
    ScriptSnapshot {
        pending: Some(PendingSequence {
            fingerprint: u64::MAX,
            remaining: f32::INFINITY,
            machine: FrozenMachine {
                body: PendingBody::Timer {
                    timer: format!("Guard.__every(0.5)#{}", u32::MAX),
                    rearm: FrozenValue::Float(f32::NAN),
                },
                registers: vec![
                    FrozenValue::Unit,
                    FrozenValue::Int(i64::MIN),
                    FrozenValue::Int(i64::MAX),
                    FrozenValue::Float(f32::INFINITY),
                    FrozenValue::Float(f32::NEG_INFINITY),
                    FrozenValue::Float(f32::NAN),
                    FrozenValue::Float(-0.0),
                    FrozenValue::Bool(true),
                    FrozenValue::Entity(target),
                    FrozenValue::Literal(String::new()),
                    FrozenValue::Literal("héllo 世界 🎮\n\"\\".into()),
                    FrozenValue::Expired,
                    FrozenValue::Vec2(Vec2::new(f32::NAN, 1.0)),
                    FrozenValue::Vec3(Vec3::new(1.0, f32::INFINITY, -3.0)),
                    FrozenValue::Vec4(Vec4::new(1.0, 2.0, 3.0, f32::NEG_INFINITY)),
                    FrozenValue::Quat(Quaternion::new(0.0, 0.0, 0.0, 1.0)),
                    FrozenValue::Color(LinearRgba::new(0.1, 0.2, 0.3, f32::NAN)),
                    FrozenValue::Null,
                ],
                frames,
                program_counter: u64::MAX,
                arguments: Vec::new(),
            },
        }),
        lifecycle: khora_core::script::InstanceLifecycle {
            resume_failed: Vec::new(),
            spawned: true,
            fault: Some(khora_core::script::RecordedFault {
                fingerprint: u64::MAX,
                reason: "héllo 世界 🎮\n\"\\".into(),
            }),
        },
        ..ScriptSnapshot::default()
    }
}

/// `value` with each entity of `map` replaced, as text: the comparison a
/// NaN register still passes, `NaN` printing as itself.
fn remapped_debug(value: &impl std::fmt::Debug, map: &HashMap<EntityId, EntityId>) -> String {
    let mut text = format!("{value:?}");
    for (from, to) in map {
        text = text.replace(&format!("{from:?}"), &format!("{to:?}"));
    }
    text
}

/// A game save taken while a schedule's body is suspended keeps every
/// register, every frame and what finishing it owes — through every encoding.
/// (A scene, and the Play snapshot, hold no observed state at all.)
#[test]
fn a_pending_sequence_with_every_register_kind_survives_every_save_encoding() {
    use khora_data::ecs::{Script, ScriptState, Transform};
    use khora_data::scene::{capture_save, compose, prepare_game, read_save_file, write_save_file};
    let mut src = World::new();
    let target = src.spawn(Transform::identity());
    let holder = src.spawn(Transform::identity());
    src.add_component(holder, Script::new("ai/guard.erg", "Guard"))
        .expect("a script attaches");
    for entity in [target, holder] {
        src.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&src).expect("the base scene captures");
    let state = ScriptState {
        behavior: "Guard".into(),
        snapshot: every_register_snapshot(target),
    };
    src.add_component(holder, state.clone())
        .expect("the observed state attaches");

    let save = capture_save(&src, AssetUUID::new(), &base).expect("the save captures");
    let mut failures = Vec::new();
    for (name, encoding) in every_encoding() {
        let file = match write_save_file(&save, encoding) {
            Ok(file) => file,
            Err(e) => {
                failures.push(format!("{name}: encode failed: {e}"));
                continue;
            }
        };
        let back = match read_save_file(&file) {
            Ok(back) => back,
            Err(e) => {
                failures.push(format!("{name}: decode of its own output failed: {e:?}"));
                continue;
            }
        };
        let mut dst = World::new();
        match prepare_game(&mut dst, &compose(&base, &back)) {
            Ok(prepared) => {
                prepared.commit(&mut dst, Identity::Keep);
            }
            Err(e) => {
                failures.push(format!("{name}: the load failed: {e}"));
                continue;
            }
        }
        let map = entity_map(&src, &dst);
        let loaded = dst
            .get::<ScriptState>(map[&holder])
            .map(|s| format!("{s:?}"));
        let expected = remapped_debug(&state, &map);
        if loaded.as_deref() != Some(expected.as_str()) {
            failures.push(format!("{name}: loaded {loaded:?}"));
        }
    }

    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

/// The snapshot schema guards a frozen machine whole: every register kind,
/// every body, every frame field — so a change to any of them changes the
/// fingerprint and refuses a stale snapshot instead of misreading it.
#[test]
fn the_snapshot_schema_traces_a_frozen_machine_whole() {
    let schema = schema_of::<khora_core::script::ScriptSnapshot>().to_string();
    assert!(!schema.contains("!incomplete"), "`{schema}`");
    for name in [
        "FrozenMachine",
        "program_counter",
        "registers",
        "frames",
        "return_pc",
        "result",
        "base",
        "function",
        "Timer",
        "rearm",
        "index",
        "Spawn",
        "Update",
        "Sequence",
        "Expired",
        "Literal",
        "Entity",
        "Quat",
        "Color",
        "Vec4",
        "Null",
    ] {
        assert!(schema.contains(name), "`{name}` is not in `{schema}`");
    }
}

/// The field `name` of the struct `record`, to change it.
fn field_mut<'a>(record: &'a mut Record, name: &str) -> &'a mut Record {
    match record {
        Record::Struct { fields, .. } => fields
            .iter_mut()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("no field `{name}`")),
        other => panic!("`{name}` looked up in {other:?}"),
    }
}

/// A save whose record holds a suspended machine in the retired tagged form
/// — the variant `Frozen` of `khora.SuspendedMachine`, written for one day on
/// the dev branch and never by a shipped engine — is not read: the load is
/// refused as a whole, the error names the `ScriptState` component, and the
/// world it was loaded into is left exactly as it was.
#[test]
fn a_save_holding_the_retired_tagged_machine_form_is_refused_whole() {
    use khora_data::ecs::{Name, Script, ScriptState, Transform};
    use khora_data::scene::record::VariantPayload;
    use khora_data::scene::{capture_save, compose, prepare_game};
    let mut src = World::new();
    let target = src.spawn(Transform::identity());
    let holder = src.spawn(Transform::identity());
    src.add_component(holder, Script::new("ai/guard.erg", "Guard"))
        .expect("a script attaches");
    for entity in [target, holder] {
        src.mark_authored(entity).expect("authored");
    }
    let base = capture_world(&src).expect("the base scene captures");
    src.add_component(
        holder,
        ScriptState {
            behavior: "Guard".into(),
            snapshot: super::sample::suspended_snapshot(target),
        },
    )
    .expect("the observed state attaches");
    let holder_id = src.persistent_id(holder).expect("an id");

    let mut save = capture_save(&src, AssetUUID::new(), &base).expect("the save captures");
    let value = value_mut(&mut save.changes, holder_id, "ScriptState");
    let pending = field_mut(field_mut(value, "snapshot"), "pending");
    let Record::Some(sequence) = pending else {
        panic!("the pending sequence is recorded as present: {pending:?}");
    };
    let machine = field_mut(sequence, "machine");
    let frozen = std::mem::replace(machine, Record::Unit);
    *machine = Record::Variant {
        enum_name: "khora.SuspendedMachine".into(),
        variant: "Frozen".into(),
        payload: VariantPayload::Newtype(Box::new(frozen)),
    };

    let mut dst = World::new();
    let resident = dst.spawn((Transform::identity(), Name::new("Resident")));
    let resident_id = dst.mark_authored(resident).expect("authored");
    let entities_before = dst.iter_entities().count();
    let components_before = components_of(&dst, resident);

    let error = match prepare_game(&mut dst, &compose(&base, &save)) {
        Ok(prepared) => {
            let applied = prepared.commit(&mut dst, Identity::Keep);
            panic!(
                "the retired tagged machine form loaded ({} entities)",
                applied.entities.len()
            )
        }
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("ScriptState"),
        "the refusal names the component: {error}"
    );
    assert_eq!(
        dst.iter_entities().count(),
        entities_before,
        "a refused load added or removed entities"
    );
    assert_eq!(
        dst.persistent_id(resident),
        Some(resident_id),
        "the resident kept its identity"
    );
    assert_eq!(
        components_of(&dst, resident),
        components_before,
        "a refused load changed the resident"
    );
    assert!(
        dst.entity_with_id(holder_id).is_none(),
        "the refused holder is in the world"
    );
}
