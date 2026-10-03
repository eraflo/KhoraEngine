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

//! Attacks on the snapshot encoding: the schema tracer, the positional codec.

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

// --- Suspended machines ----------------------------------------------------

fn machines() -> Vec<khora_core::script::SuspendedMachine> {
    use khora_core::script::{FrozenMachine, FrozenValue, PendingBody, SuspendedMachine};
    vec![
        SuspendedMachine::Legacy(vec![]),
        SuspendedMachine::Legacy(vec![0, 7, 255]),
        SuspendedMachine::Frozen(FrozenMachine {
            body: PendingBody::Sequence,
            registers: vec![
                FrozenValue::Int(-3),
                FrozenValue::Float(0.5),
                FrozenValue::Literal("é".into()),
            ],
            frames: vec![],
            program_counter: 9,
        }),
    ]
}

/// Every form, through a record and every self-describing carrier of one,
/// through serde_json and rmp directly, and by position.
#[test]
fn a_suspended_machine_round_trips_everywhere() {
    use khora_core::script::SuspendedMachine;
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
            from_record::<SuspendedMachine>(&record, &mut NoRefs).as_ref(),
            Ok(&machine)
        );
        let json: Record = serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(
            from_record::<SuspendedMachine>(&json, &mut NoRefs).as_ref(),
            Ok(&machine),
            "json record"
        );
        let mp: Record = rmp_serde::from_slice(&rmp_serde::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            from_record::<SuspendedMachine>(&mp, &mut NoRefs).as_ref(),
            Ok(&machine),
            "msgpack record"
        );
        let direct_json: SuspendedMachine =
            serde_json::from_str(&serde_json::to_string(&machine).unwrap()).unwrap();
        assert_eq!(direct_json, machine, "direct json");
        let direct_mp: Result<SuspendedMachine, _> =
            rmp_serde::from_slice(&rmp_serde::to_vec(&machine).unwrap());
        assert_eq!(direct_mp.ok().as_ref(), Some(&machine), "direct msgpack");
        assert_eq!(round_trip(&machine).as_ref(), Ok(&machine), "positional");
    }
}

/// The untagged form records held before the tag: a frozen machine as its
/// struct, a legacy one as its bytes — read back through every carrier.
#[test]
fn an_untagged_machine_record_still_loads() {
    use khora_core::script::SuspendedMachine;
    use khora_data::scene::record::{from_record, to_record, Record};
    struct Nothing;
    impl ReferenceWriter for Nothing {
        fn write_entity(&mut self, _e: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    for machine in machines() {
        let old = match &machine {
            SuspendedMachine::Legacy(bytes) => to_record(bytes, &mut Nothing),
            SuspendedMachine::Frozen(frozen) => to_record(frozen, &mut Nothing),
        }
        .expect("record");
        assert_eq!(
            from_record::<SuspendedMachine>(&old, &mut NoRefs).as_ref(),
            Ok(&machine),
            "{old:?}"
        );
        let json: Record = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(
            from_record::<SuspendedMachine>(&json, &mut NoRefs).as_ref(),
            Ok(&machine),
            "json {json:?}"
        );
        let mp: Record = rmp_serde::from_slice(&rmp_serde::to_vec(&old).unwrap()).unwrap();
        assert_eq!(
            from_record::<SuspendedMachine>(&mp, &mut NoRefs).as_ref(),
            Ok(&machine),
            "msgpack {mp:?}"
        );
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
