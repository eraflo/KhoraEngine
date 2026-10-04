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

//! Inputs and sequences a scene load must survive: records a tool or a
//! damaged file could hold, and worlds a load is brought beside.

use khora_data::ecs::{Camera, Name, Parent, Transform, World};
use khora_data::scene::record::{EntityRef, ReferenceWriter};
use khora_data::scene::{component_to_record, registration_named};

use super::*;

/// Writes an entity by the identity `world` knows it by.
struct ById<'w>(&'w World);

impl ReferenceWriter for ById<'_> {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.0
            .persistent_id(entity)
            .map_or(EntityRef::Outside, EntityRef::Id)
    }
}

/// The name an entity of `world` carries.
fn name_of(world: &World, entity: EntityId) -> Option<String> {
    world.get::<Name>(entity).map(|n| n.as_str().to_owned())
}

/// The loaded entity named `name`.
fn named(world: &World, name: &str) -> EntityId {
    world
        .iter_entities()
        .find(|e| name_of(world, *e).as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no entity named {name}"))
}

/// `Children` is derived from `Parent`; a record that also carries a
/// `Children` column (a tool's export, a hand-edited text save) must not be
/// able to put a parent's list out of step with its children — nor push two
/// values into one column of the page it builds. Two parents with a recorded
/// (empty) `Children` each, and a child each pointing at them: after the
/// load, each parent lists exactly its own child.
#[test]
fn a_recorded_children_column_cannot_corrupt_the_hierarchy() {
    let mut src = World::new();
    let p1 = src.spawn((Transform::identity(), Name::new("P1")));
    let p2 = src.spawn((Transform::identity(), Name::new("P2")));
    let c1 = src.spawn((Transform::identity(), Name::new("C1")));
    let c2 = src.spawn((Transform::identity(), Name::new("C2")));
    assert!(src.set_parent(c1, Some(p1)));
    assert!(src.set_parent(c2, Some(p2)));
    for e in [p1, p2, c1, c2] {
        src.mark_authored(e).expect("authored");
    }

    // The record form of an empty `Children`, as its registration writes it.
    let mut scratch = World::new();
    let lone = scratch.spawn((Transform::identity(), Name::new("lone")));
    scratch
        .add_component(lone, Children(Vec::new()))
        .expect("children attach");
    let children_reg = registration_named("Children").expect("Children is registered");
    let empty_children = component_to_record(children_reg, &scratch, lone, &mut ById(&scratch))
        .expect("lone has children")
        .expect("children write");

    let mut record = capture_world(&src).expect("captures");
    let p1_id = src.persistent_id(p1).expect("id");
    let parents_page = slot_of(&record, p1_id, "Name").expect("P1 has a row").0;
    assert!(
        record.pages[parents_page]
            .rows
            .contains(&src.persistent_id(p2).expect("id")),
        "P1 and P2 share a page record"
    );
    add_column(&mut record, parents_page, "Children", empty_children);

    let mut dst = World::new();
    match apply(&mut dst, &record, Identity::Keep) {
        Err(_) => {} // Refusing a derived column is an acceptable answer.
        Ok(_) => {
            let mut wrong = Vec::new();
            for (parent, child) in [("P1", "C1"), ("P2", "C2")] {
                let (p, c) = (named(&dst, parent), named(&dst, child));
                let listed = dst.get::<Children>(p).map(|list| list.0.clone());
                if dst.get::<Parent>(c).map(|x| x.0) != Some(p) || listed != Some(vec![c]) {
                    wrong.push(format!("{parent} (child {child} = {c:?}) lists {listed:?}"));
                }
            }
            assert!(
                wrong.is_empty(),
                "each parent must list exactly its child: {wrong:?}"
            );
        }
    }
}

/// A float an f32 can hold — an infinite far plane, a NaN a simulation
/// produced — saves and loads back in every encoding, or the save refuses
/// it. A text save that writes it as something the load then refuses, or
/// reads back as something else, loses a scene that saved without error.
#[test]
fn a_non_finite_float_survives_every_encoding() {
    let mut src = World::new();
    let eye = src.spawn((Transform::identity(), Name::new("Eye")));
    src.add_component(
        eye,
        Camera::new_perspective(std::f32::consts::FRAC_PI_3, 1.5, 0.05, f32::INFINITY),
    )
    .expect("a camera attaches");
    let id = src.mark_authored(eye).expect("authored");

    let mut lost = Vec::new();
    for (name, encoding) in every_encoding() {
        let record = capture_world(&src).expect("captures");
        let Ok(bytes) = encoding.encode(&record) else {
            continue; // Refusing to save it is an honest answer.
        };
        let back = match encoding.decode(&bytes) {
            Ok(back) => back,
            Err(e) => {
                lost.push(format!("{name}: its own output does not decode: {e}"));
                continue;
            }
        };
        let mut dst = World::new();
        if let Err(e) = apply(&mut dst, &back, Identity::Keep) {
            lost.push(format!("{name}: saved without error, does not load: {e}"));
            continue;
        }
        let twin = dst.entity_with_id(id).expect("loaded");
        let far = dst.get::<Camera>(twin).map(|c| c.z_far);
        if far != Some(f32::INFINITY) {
            lost.push(format!("{name}: the far plane came back as {far:?}"));
        }
    }
    assert!(lost.is_empty(), "{lost:#?}");
}

/// A load beside a world keeps every saved identity that no entity outside
/// the load holds. Here only the first recorded identity is taken by an
/// entity already in the world — one identified before the load; the second
/// is free, and must stay with the entity it was saved for — not be given to
/// its sibling in the load.
#[test]
fn a_load_beside_keeps_every_created_id_nothing_else_holds() {
    let mut src = World::new();
    let a = src.spawn((Transform::identity(), Name::new("A")));
    let b = src.spawn((Transform::identity(), Name::new("B")));
    // Asked in spawn order, so A is numbered first, as a resident will be.
    src.persistent_id(a).expect("id");
    let b_id = src.persistent_id(b).expect("id");
    assert!(b_id.is_created());
    let record = capture_world(&src).expect("captures");

    let mut dst = World::new();
    let resident = dst.spawn((Transform::identity(), Name::new("Resident")));
    // Identified before the load: its identity is taken.
    let resident_id = dst.persistent_id(resident).expect("id");
    assert_eq!(
        record.entities[0], resident_id,
        "the first recorded identity is the resident's"
    );
    assert_ne!(b_id, resident_id);

    apply(&mut dst, &record, Identity::Keep).expect("loads");

    assert_eq!(
        dst.entity_with_id(resident_id),
        Some(resident),
        "the resident keeps its own identity"
    );
    let holder = dst
        .entity_with_id(b_id)
        .expect("B's identity is held by someone");
    assert_eq!(
        name_of(&dst, holder).as_deref(),
        Some("B"),
        "B's saved identity, free in this world, names another entity"
    );
}

/// The compact encoding writes every name once, and a value refers to it by
/// number — so a reader must not let one small reference stand for a large
/// allocation. A 16 KiB name referred to by 4096 two-byte values is a 25 KiB
/// file; decoding it must not turn into 64 MiB of copies (scaled up, a few
/// megabytes of file would abort the process on allocation).
#[test]
fn a_compact_name_reference_cannot_amplify_into_a_huge_allocation() {
    const NAME_LEN: usize = 16 * 1024;
    const REFS: usize = 4096;
    let mut bytes = vec![1u8]; // layout version
    varint(&mut bytes, 1); // one symbol
    varint(&mut bytes, NAME_LEN as u64);
    bytes.extend(std::iter::repeat_n(b'n', NAME_LEN));
    varint(&mut bytes, 0); // no shapes
    varint(&mut bytes, 1); // one entity
    bytes.extend_from_slice(&7u64.to_le_bytes());
    varint(&mut bytes, 1); // one page
    varint(&mut bytes, 1); // one component: symbol 0
    varint(&mut bytes, 0);
    varint(&mut bytes, 1); // one row
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.push(12); // SEQ
    varint(&mut bytes, REFS as u64);
    for _ in 0..REFS {
        bytes.push(14); // UNIT_STRUCT
        varint(&mut bytes, 0); // named by symbol 0
    }

    let Ok(record) = CompactEncoding.decode(&bytes) else {
        return; // Refusing it is fine.
    };
    let decoded: usize = record
        .pages
        .iter()
        .flat_map(|page| page.columns.iter().flatten())
        .map(heap_bytes)
        .sum();
    assert!(
        decoded <= bytes.len() * 64,
        "a {}-byte file decoded into {decoded} bytes of names",
        bytes.len()
    );
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The bytes of text a record holds, roughly.
fn heap_bytes(record: &Record) -> usize {
    use khora_data::scene::record::VariantPayload;
    match record {
        Record::Str(s) => s.len(),
        Record::UnitStruct { name } => name.len(),
        Record::Struct { name, fields } => {
            name.len()
                + fields
                    .iter()
                    .map(|(f, v)| f.len() + heap_bytes(v))
                    .sum::<usize>()
        }
        Record::TupleStruct { name, fields } => {
            name.len() + fields.iter().map(heap_bytes).sum::<usize>()
        }
        Record::Newtype { name, value } => name.len() + heap_bytes(value),
        Record::Seq(items) => items.iter().map(heap_bytes).sum(),
        Record::Map(entries) => entries
            .iter()
            .map(|(k, v)| heap_bytes(k) + heap_bytes(v))
            .sum(),
        Record::Some(inner) => heap_bytes(inner),
        Record::Variant {
            enum_name,
            variant,
            payload,
        } => {
            enum_name.len()
                + variant.len()
                + match payload {
                    VariantPayload::Unit => 0,
                    VariantPayload::Newtype(inner) => heap_bytes(inner),
                    VariantPayload::Tuple(items) => items.iter().map(heap_bytes).sum(),
                    VariantPayload::Struct(fields) => {
                        fields.iter().map(|(f, v)| f.len() + heap_bytes(v)).sum()
                    }
                }
        }
        _ => 0,
    }
}

/// A MessagePack save nested as deep as the format's reader allows must be
/// refused as an error, not overflow the stack of the thread loading it
/// (2 MiB, a test thread's default; a Windows main thread has 1 MiB).
#[test]
fn a_deeply_nested_msgpack_value_is_an_error_not_a_crash() {
    let record = SceneRecord {
        entities: vec![PersistentId::authored(7)],
        instances: Vec::new(),
        pages: vec![PageRecord {
            components: vec!["Transform".into()],
            rows: vec![PersistentId::authored(7)],
            columns: vec![vec![Record::Str("PLACEHOLDER".into())]],
        }],
    };
    let bytes = MsgPackEncoding.encode(&record).expect("encodes");
    let marker = b"\xabPLACEHOLDER";
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("the placeholder is in the bytes");
    let mut nested = bytes[..at].to_vec();
    nested.extend(std::iter::repeat_n(0x91u8, 1000)); // [ [ [ ...
    nested.push(0xc0); // nil
    nested.extend_from_slice(&bytes[at + marker.len()..]);

    let outcome = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || MsgPackEncoding.decode(&nested).map(|_| ()))
        .expect("spawns")
        .join();
    assert!(outcome.is_ok(), "decoding panicked");
}

/// Coverage: `Children` is built from every `Parent` before any page is
/// placed, so a parent whose page record comes after its child's still lists
/// the child.
#[test]
fn a_parent_recorded_after_its_child_still_lists_it() {
    let mut src = World::new();
    let p = src.spawn((Transform::identity(), Name::new("P")));
    let c = src.spawn((Transform::identity(), Name::new("C")));
    assert!(src.set_parent(c, Some(p)));
    let mut record = capture_world(&src).expect("captures");
    record
        .pages
        .sort_by_key(|page| !page.components.iter().any(|n| n == "Parent"));
    let p_id = src.persistent_id(p).unwrap();
    let c_id = src.persistent_id(c).unwrap();
    assert!(
        slot_of(&record, c_id, "Parent").unwrap().0 < slot_of(&record, p_id, "Name").unwrap().0
    );
    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("loads");
    let (p2, c2) = (named(&dst, "P"), named(&dst, "C"));
    assert_eq!(dst.get::<Children>(p2).map(|l| l.0.clone()), Some(vec![c2]));
}

/// Coverage: a parent with no page in the hierarchy's domain gains its
/// `Children` on its own (the `add_component` branch of the commit) — listed,
/// its other components kept, and no row left for compaction.
#[test]
fn a_parent_without_a_spatial_page_gains_its_children_without_migrating() {
    let mut src = World::new();
    let p = src.spawn(Camera::new_perspective(1.0, 1.0, 0.1, 10.0));
    let c = src.spawn((Transform::identity(), Name::new("C")));
    assert!(src.set_parent(c, Some(p)));
    let p_id = src.mark_authored(p).unwrap();
    let mut dst = World::new();
    let record = capture_world(&src).expect("captures");
    apply(&mut dst, &record, Identity::Keep).expect("loads");
    let p2 = dst.entity_with_id(p_id).unwrap();
    let c2 = named(&dst, "C");
    assert_eq!(dst.get::<Children>(p2).map(|l| l.0.clone()), Some(vec![c2]));
    assert!(dst.get::<Camera>(p2).is_some());
    let mut maintenance = khora_data::ecs::EcsMaintenance::with_budget(usize::MAX);
    maintenance.tick(&mut dst);
    assert_eq!(maintenance.last_compacted_count(), 0);
}

/// Coverage: a `Parent` loop — an entity its own parent, two entities each
/// other's — is refused as "its own ancestor", the world is untouched, and
/// the ids it reserved are back in the pool for the next load.
#[test]
fn a_parent_cycle_is_refused_and_the_world_untouched() {
    let mut src = World::new();
    let a = src.spawn((Transform::identity(), Name::new("A")));
    let b = src.spawn((Transform::identity(), Name::new("B")));
    assert!(src.set_parent(b, Some(a)));
    let a_id = src.persistent_id(a).unwrap();
    let b_id = src.persistent_id(b).unwrap();
    let base = capture_world(&src).expect("captures");

    let mut self_parent = base.clone();
    replace_entity(value_mut(&mut self_parent, b_id, "Parent"), b_id);
    let mut two_cycle = base.clone();
    let parent_of_b = value_mut(&mut two_cycle, b_id, "Parent").clone();
    let mut parent_of_a = parent_of_b.clone();
    replace_entity(&mut parent_of_a, b_id);
    let page = isolate(&mut two_cycle, a_id, "Name");
    add_column(&mut two_cycle, page, "Parent", parent_of_a);

    for (case, record) in [("self", self_parent), ("two", two_cycle)] {
        let mut world = super::failures::occupied_world();
        let before = super::failures::fingerprint(&world);
        let failure = apply(&mut world, &record, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{case}: a cycle must be refused"));
        assert!(
            failure.message.contains("ancestor"),
            "{case}: {}",
            failure.message
        );
        assert_eq!(super::failures::fingerprint(&world), before, "{case}");
        // Then a good load still works and reuses the released slots.
        let n = world.iter_entities().count();
        apply(&mut world, &base, Identity::Keep).expect("good load");
        assert_eq!(world.iter_entities().count(), n + 2);
    }
}

/// Points every entity reference inside `record` at `to`.
fn replace_entity(record: &mut Record, to: PersistentId) {
    use khora_data::scene::record::VariantPayload;
    match record {
        Record::Entity(r) => *r = EntityRef::Id(to),
        Record::Some(inner) | Record::Newtype { value: inner, .. } => replace_entity(inner, to),
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            items.iter_mut().for_each(|i| replace_entity(i, to))
        }
        Record::Struct { fields, .. } => fields.iter_mut().for_each(|(_, v)| replace_entity(v, to)),
        Record::Variant {
            payload: VariantPayload::Newtype(inner),
            ..
        } => replace_entity(inner, to),
        _ => {}
    }
}

/// Coverage: a failed load gives back every id it reserved — a hundred
/// failed loads of eight entities leave the store's slots for the next spawn.
#[test]
fn failed_loads_give_back_every_reserved_slot() {
    let mut src = World::new();
    for i in 0..8 {
        src.spawn((Transform::identity(), Name::new(format!("e{i}"))));
    }
    let mut bad = capture_world(&src).expect("captures");
    let first = bad.entities[0];
    *value_mut(&mut bad, first, "Transform") = Record::Str("nope".into());
    let mut world = World::new();
    for _ in 0..100 {
        assert!(apply(&mut world, &bad, Identity::Keep).is_err());
    }
    let spawned = world.spawn(Transform::identity());
    assert!(spawned.index < 16, "{spawned:?}");
}

/// Coverage: the `Identity::Keep` collision path. The same authored scene
/// loaded twice into one world: the second copy's identities are taken, so
/// it gets fresh ones — four distinct ids — and each copy's hierarchy stays
/// within itself.
#[test]
fn the_same_scene_loaded_twice_beside_itself_takes_fresh_ids() {
    let mut src = World::new();
    let p = src.spawn((Transform::identity(), Name::new("P")));
    let c = src.spawn((Transform::identity(), Name::new("C")));
    assert!(src.set_parent(c, Some(p)));
    src.mark_authored(p).unwrap();
    src.mark_authored(c).unwrap();
    let record = capture_world(&src).expect("captures");
    let mut dst = World::new();
    let first = apply(&mut dst, &record, Identity::Keep).expect("first");
    let second = apply(&mut dst, &record, Identity::Keep).expect("second");
    assert_eq!(dst.iter_entities().count(), 4);
    let ids: std::collections::HashSet<_> = dst
        .iter_entities()
        .map(|e| dst.persistent_id(e).unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    for applied in [&first, &second] {
        let p2 = applied.entities[0].1;
        let c2 = applied.entities[1].1;
        assert_eq!(dst.get::<Parent>(c2).map(|x| x.0), Some(p2));
        assert_eq!(dst.get::<Children>(p2).map(|l| l.0.clone()), Some(vec![c2]));
    }
}

/// Coverage: a subtree whose child was despawned (its index recycled by a
/// later spawn) captures only the live part and loads.
#[test]
fn a_subtree_with_a_despawned_child_captures_the_live_part() {
    let mut src = World::new();
    let p = src.spawn((Transform::identity(), Name::new("P")));
    let c = src.spawn((Transform::identity(), Name::new("C")));
    let d = src.spawn((Transform::identity(), Name::new("D")));
    assert!(src.set_parent(c, Some(p)));
    assert!(src.set_parent(d, Some(p)));
    src.despawn(c);
    // Recycle c's index.
    let _reuse = src.spawn((Transform::identity(), Name::new("R")));
    let record = khora_data::scene::capture_subtree(&src, p).expect("captures");
    assert_eq!(record.entities.len(), 2, "{:?}", record.entities);
    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Fresh).expect("loads");
    assert_eq!(dst.iter_entities().count(), 2);
}

/// Coverage: a text decimal no `f32` can hold (`1e39`), read into an `f32`
/// field, is refused rather than loaded as infinity.
#[test]
fn a_decimal_beyond_f32_range_is_refused() {
    let mut src = World::new();
    let eye = src.spawn(Camera::new_perspective(1.0, 1.0, 0.1, 10.0));
    let id = src.mark_authored(eye).unwrap();
    let record = capture_world(&src).expect("captures");
    let text = String::from_utf8(TextEncoding.encode(&record).unwrap()).unwrap();
    assert!(text.contains("\"z_far\": 10.0"), "{text}");
    let text = text.replace("\"z_far\": 10.0", "\"z_far\": 1e39");
    let back = TextEncoding.decode(text.as_bytes()).expect("decodes");
    let mut dst = World::new();
    let r = apply(&mut dst, &back, Identity::Keep);
    let far = r.map(|_| {
        dst.get::<Camera>(dst.entity_with_id(id).expect("loaded"))
            .map(|c| c.z_far)
    });
    assert!(far.is_err(), "loaded with z_far = {far:?}");
}
