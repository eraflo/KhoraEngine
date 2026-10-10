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

//! A tuple component with a `#[component(skip)]` field.
//!
//! A tuple field has no name: the derive addresses it by position. The
//! `Serializable` mirror, the field schema and the script slots hold only the
//! included fields, so an included field's *slot* (its index among the
//! included) and its *position* (its index in the struct) differ as soon as a
//! skipped field comes before it. Each must be read from its own position and
//! written back to it; a skipped field takes its `Default` on the way back.
//!
//! The components live in their own test target: they are the derive's tuple
//! form with a skip, and a derive that cannot expand them must not take the
//! rest of the suite down with it. As in `derive_paths.rs`, the two modules
//! below rebuild the `crate::ecs` / `crate::scene` paths the derive expands to.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ErgonType, ScriptValue};
use khora_data::ecs::component::Component as _;
use khora_data::ecs::{ComponentKey, World};
use khora_data::scene::{
    apply, capture_world, CompactEncoding, ComponentShape, FieldSchema, Identity, MsgPackEncoding,
    SceneEncoding, TextEncoding,
};
use khora_macros::Component;

/// The `crate::ecs` the derive expands against.
mod ecs {
    pub use khora_data::ecs::*;
}

/// The `crate::scene` the derive expands against.
mod scene {
    pub use khora_data::scene::*;
}

/// A skipped field between two included ones: the second included field
/// (slot 1) is the third of the struct (position 2).
#[derive(Debug, Clone, Default, PartialEq, Component)]
#[component(domain = Spatial)]
pub struct GappedPair(pub f32, #[component(skip)] pub u32, pub f32);

/// A skipped field first: the only included field (slot 0) is the second of
/// the struct (position 1).
#[derive(Debug, Clone, Default, PartialEq, Component)]
#[component(domain = Spatial)]
pub struct CachedLead(#[component(skip)] pub u32, pub f32);

/// A skipped field last: the slots match the positions, but the struct is
/// still one field longer than its mirror.
#[derive(Debug, Clone, Default, PartialEq, Component)]
#[component(domain = Spatial)]
pub struct CachedTail(pub f32, #[component(skip)] pub u32);

/// The three encodings, each with the name a failure message uses for it.
fn every_encoding() -> [(&'static str, &'static dyn SceneEncoding); 3] {
    [
        ("compact", &CompactEncoding),
        ("text", &TextEncoding),
        ("msgpack", &MsgPackEncoding),
    ]
}

/// Captures `src`, writes it in `encoding`, reads it back and applies it to a
/// fresh world with the saved identities.
fn reload(src: &World, name: &str, encoding: &dyn SceneEncoding) -> World {
    let record = capture_world(src).unwrap_or_else(|e| panic!("{name}: capture failed: {e}"));
    let bytes = encoding
        .encode(&record)
        .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
    let back = encoding
        .decode(&bytes)
        .unwrap_or_else(|e| panic!("{name}: decode of its own output failed: {e}"));
    let mut dst = World::new();
    let applied = apply(&mut dst, &back, Identity::Keep)
        .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
    assert!(
        applied.report.is_clean(),
        "{name}: a same-version round trip adapted something: {:?}",
        applied.report.entries
    );
    dst
}

/// The entity of `dst` known by the persistent identity `entity` has in `src`.
fn twin(src: &World, dst: &World, entity: EntityId) -> EntityId {
    let id = src.persistent_id(entity).expect("a live entity has an id");
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("no entity known as {id:?} after the load"))
}

/// The slot `world` gives field `name` of component `key`.
fn slot(world: &World, key: ComponentKey, name: &str) -> Option<usize> {
    world
        .components()
        .vtable(key)
        .expect("a registered component")
        .columns
        .field_slot(name)
}

// ─── The serializable mirror ────────────────────────────────────────────────

/// **The mirror holds the included fields, from their own positions; the way
/// back puts them where they were and defaults the skipped one.**
#[test]
fn a_tuple_mirror_round_trip_keeps_included_fields_and_defaults_a_skipped_middle_field() {
    let mirror = SerializableGappedPair::from(GappedPair(1.5, 7, 2.5));
    assert_eq!(
        serde_json::to_value(&mirror).expect("the mirror is JSON"),
        serde_json::json!([1.5, 2.5]),
        "the mirror holds the first and the third field, not the skipped second"
    );
    assert_eq!(GappedPair::from(mirror), GappedPair(1.5, 0, 2.5));
}

/// The same with the skipped field first: the mirror's only field is the
/// struct's second. A mirror of one field is a newtype, which serde writes as
/// its inner value.
#[test]
fn a_tuple_mirror_round_trip_defaults_a_skipped_leading_field() {
    let mirror = SerializableCachedLead::from(CachedLead(7, 2.5));
    assert_eq!(
        serde_json::to_value(&mirror).expect("the mirror is JSON"),
        serde_json::json!(2.5)
    );
    assert_eq!(CachedLead::from(mirror), CachedLead(0, 2.5));
}

/// And with the skipped field last: the way back still builds the whole
/// struct.
#[test]
fn a_tuple_mirror_round_trip_defaults_a_skipped_trailing_field() {
    let mirror = SerializableCachedTail::from(CachedTail(1.5, 7));
    assert_eq!(
        serde_json::to_value(&mirror).expect("the mirror is JSON"),
        serde_json::json!(1.5)
    );
    assert_eq!(CachedTail::from(mirror), CachedTail(1.5, 0));
}

/// The registration's JSON pair — what the editor inspector reads and writes —
/// goes through the same mirror, on a `World`.
#[test]
fn a_tuple_registration_json_round_trip_defaults_the_skipped_field() {
    let registration =
        khora_data::scene::registration_of("GappedPair").expect("the derive registers GappedPair");
    let mut world = World::new();
    let entity = world.spawn(GappedPair(1.5, 7, 2.5));

    let json = (registration.to_json)(&world, entity).expect("a present component has JSON");
    assert_eq!(json, serde_json::json!([1.5, 2.5]));

    let fresh = world.spawn(());
    (registration.from_json)(&mut world, fresh, &json).expect("the JSON it wrote reads back");
    assert_eq!(
        world.clone_component::<GappedPair>(fresh),
        Some(GappedPair(1.5, 0, 2.5))
    );

    // Written in place over a present component, the skipped field is reset
    // too: the JSON does not carry it.
    (registration.from_json)(&mut world, entity, &serde_json::json!([4.0, 5.0]))
        .expect("an in-place write succeeds");
    assert_eq!(
        world.clone_component::<GappedPair>(entity),
        Some(GappedPair(4.0, 0, 5.0))
    );
}

// ─── Scene records ──────────────────────────────────────────────────────────

/// **A scene keeps a tuple component's included fields and none of its
/// skipped ones**, through every encoding: each included field comes back at
/// its own position, each skipped field as its `Default`.
#[test]
fn a_tuple_component_with_a_skipped_field_round_trips_through_every_encoding() {
    let mut src = World::new();
    let gapped = src.spawn(GappedPair(1.5, 7, 2.5));
    let lead = src.spawn(CachedLead(9, 3.5));
    let tail = src.spawn(CachedTail(4.5, 11));
    for entity in [gapped, lead, tail] {
        src.mark_authored(entity)
            .expect("a live entity can be authored");
    }

    for (name, encoding) in every_encoding() {
        let dst = reload(&src, name, encoding);
        assert_eq!(
            dst.clone_component::<GappedPair>(twin(&src, &dst, gapped)),
            Some(GappedPair(1.5, 0, 2.5)),
            "{name}"
        );
        assert_eq!(
            dst.clone_component::<CachedLead>(twin(&src, &dst, lead)),
            Some(CachedLead(0, 3.5)),
            "{name}"
        );
        assert_eq!(
            dst.clone_component::<CachedTail>(twin(&src, &dst, tail)),
            Some(CachedTail(4.5, 0)),
            "{name}"
        );
    }
}

// ─── Schema and script slots ────────────────────────────────────────────────

/// **The schema names the included fields by slot** — `"0"`, `"1"` — with
/// their own types, and the skipped field is not in it.
#[test]
fn a_tuple_schema_names_included_fields_by_slot() {
    let registration =
        khora_data::scene::registration_of("GappedPair").expect("the derive registers GappedPair");
    assert_eq!(
        registration.shape,
        ComponentShape::Fields(&[
            FieldSchema {
                name: "0",
                ty: "f32",
            },
            FieldSchema {
                name: "1",
                ty: "f32",
            },
        ])
    );
    assert_eq!(GappedPair::script_fields(), ["0", "1"]);
    assert_eq!(GappedPair::script_type(0), Some(ErgonType::Float));
    assert_eq!(GappedPair::script_type(1), Some(ErgonType::Float));
    assert_eq!(GappedPair::script_type(2), None, "no third slot");

    let lead =
        khora_data::scene::registration_of("CachedLead").expect("the derive registers CachedLead");
    assert_eq!(
        lead.shape,
        ComponentShape::Fields(&[FieldSchema {
            name: "0",
            ty: "f32",
        }])
    );
    assert_eq!(CachedLead::script_fields(), ["0"]);
    assert_eq!(CachedLead::script_type(0), Some(ErgonType::Float));
}

/// **A slot reads and writes the field at its position**: slot 1 of
/// `GappedPair` is its third field, slot 0 of `CachedLead` its second.
#[test]
fn a_tuple_slot_reads_and_writes_the_field_at_its_position() {
    let mut pair = GappedPair(1.5, 7, 2.5);
    assert_eq!(pair.read_field(0), Some(ScriptValue::Float(1.5)));
    assert_eq!(pair.read_field(1), Some(ScriptValue::Float(2.5)));
    assert_eq!(pair.read_field(2), None);
    pair.write_field(1, &ScriptValue::Float(8.0))
        .expect("slot 1 takes a float");
    assert_eq!(pair, GappedPair(1.5, 7, 8.0), "the skipped field untouched");

    let mut lead = CachedLead(9, 3.5);
    assert_eq!(lead.read_field(0), Some(ScriptValue::Float(3.5)));
    lead.write_field(0, &ScriptValue::Float(6.0))
        .expect("slot 0 takes a float");
    assert_eq!(lead, CachedLead(9, 6.0));
}

/// The same through a `World` row, by the slot the component's column
/// operations give each name.
#[test]
fn a_tuple_row_reads_and_writes_the_field_at_its_position() {
    let mut world = World::new();
    let entity = world.spawn((GappedPair(1.5, 7, 2.5), CachedLead(9, 3.5)));

    let pair = ComponentKey::of::<GappedPair>();
    assert_eq!(slot(&world, pair, "0"), Some(0));
    assert_eq!(slot(&world, pair, "1"), Some(1));
    assert_eq!(
        slot(&world, pair, "2"),
        None,
        "the skipped field has no slot"
    );
    let row = world.row(entity, pair).expect("the entity holds it");
    assert_eq!(row.field(0), Some(ScriptValue::Float(1.5)));
    assert_eq!(row.field(1), Some(ScriptValue::Float(2.5)));

    world
        .row_mut(entity, pair)
        .expect("the entity holds it")
        .set_field(1, &ScriptValue::Float(8.0))
        .expect("slot 1 takes a float");
    assert_eq!(
        world.clone_component::<GappedPair>(entity),
        Some(GappedPair(1.5, 7, 8.0))
    );

    let lead = ComponentKey::of::<CachedLead>();
    assert_eq!(slot(&world, lead, "0"), Some(0));
    let row = world.row(entity, lead).expect("the entity holds it");
    assert_eq!(row.field(0), Some(ScriptValue::Float(3.5)));
}
