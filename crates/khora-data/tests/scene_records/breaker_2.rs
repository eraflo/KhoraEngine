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

//! Second round: the fixes for the first round's findings, attacked.

use khora_core::script::ScriptValue;
use khora_data::ecs::{Camera, Name, Parent, Script, Transform, World};

use super::*;

fn nested_array(depth: usize) -> ScriptValue {
    let mut v = ScriptValue::Int(1);
    for _ in 0..depth {
        v = ScriptValue::Array(vec![v]);
    }
    v
}

fn deep_world(depth: usize) -> World {
    let mut src = World::new();
    src.spawn((
        Transform::identity(),
        Name::new("deep"),
        Script {
            module: "a.erg".into(),
            behavior: "A".into(),
            fields: vec![("v".into(), nested_array(depth))],
            runtime: Default::default(),
        },
    ));
    src
}

/// The compact name budget must cover every name the decoder copies out of
/// the symbol table — a page's component names included. A 16 KiB name
/// listed 4096 times as one page's components, with no rows (so no value
/// ever touches the budget), is a 20 KiB file; decoding must not make 64 MiB
/// of copies of it.
#[test]
fn a_compact_page_signature_cannot_amplify_a_name_either() {
    const NAME_LEN: usize = 16 * 1024;
    const REFS: usize = 4096;
    let mut bytes = vec![1u8]; // layout version
    varint(&mut bytes, 1); // one symbol
    varint(&mut bytes, NAME_LEN as u64);
    bytes.extend(std::iter::repeat_n(b'n', NAME_LEN));
    varint(&mut bytes, 0); // no shapes
    varint(&mut bytes, 0); // no entities
    varint(&mut bytes, 1); // one page
    varint(&mut bytes, REFS as u64); // its components ...
    for _ in 0..REFS {
        varint(&mut bytes, 0); // ... each symbol 0
    }
    varint(&mut bytes, 0); // no rows, so no values

    let Ok(record) = CompactEncoding.decode(&bytes) else {
        return; // Refusing it is fine.
    };
    let decoded: usize = record
        .pages
        .iter()
        .flat_map(|page| page.components.iter())
        .map(String::len)
        .sum();
    assert!(
        decoded <= bytes.len() * 64,
        "a {}-byte file decoded into {decoded} bytes of component names",
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

/// A script field nested as deep as a capture accepts (an array 62 deep is
/// about 125 record levels; 63 is refused by the capture) saves without
/// error in every encoding — so it must load in every encoding. The text
/// encoding writes it, then its JSON reader stops at serde_json's own
/// recursion limit (128 nested arrays and maps, counting the five levels of
/// the scene document around the value) below the record's.
#[test]
fn what_a_capture_accepts_loads_back_in_every_encoding() {
    let record = capture_world(&deep_world(62)).expect("a capture accepts it");
    let mut lost = Vec::new();
    for (name, encoding) in every_encoding() {
        let Ok(bytes) = encoding.encode(&record) else {
            continue; // Refusing to save it is an honest answer.
        };
        match encoding.decode(&bytes) {
            Err(e) => lost.push(format!("{name}: saved, does not decode: {e}")),
            Ok(back) => {
                if let Err(e) = apply(&mut World::new(), &back, Identity::Keep) {
                    lost.push(format!("{name}: saved, does not load: {e}"));
                }
            }
        }
    }
    assert!(lost.is_empty(), "{lost:#?}");
}

/// The editor loads scenes, and duplicates or instantiates prefabs through
/// the compact encoding, on the main thread — 1 MiB of stack on Windows, in
/// the debug build `cargo hub-dev` runs. Decoding what a capture accepts must
/// fit in it: the compact reader's frame per level times its depth bound
/// (128) does not.
#[test]
fn compact_decode_at_the_depth_a_capture_accepts_fits_a_1_mib_stack() {
    let record = capture_world(&deep_world(62)).expect("a capture accepts it");
    let bytes = CompactEncoding.encode(&record).expect("encodes");
    let outcome = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || CompactEncoding.decode(&bytes).map(|_| ()))
        .expect("spawns")
        .join();
    assert!(
        matches!(outcome, Ok(Ok(()))),
        "decoding failed: {outcome:?}"
    );
}

/// Coverage: `Keep` beside a world holding both kinds of identity. Each
/// recorded id taken by a resident gives way to a fresh id of its kind; each
/// free one is kept; later spawns take none of them.
#[test]
fn keep_beside_mixed_collisions_gives_way_by_kind() {
    let mut src = World::new();
    let a = src.spawn((Transform::identity(), Name::new("A"))); // authored, taken
    let b = src.spawn((Transform::identity(), Name::new("B"))); // created(1), taken
    let c = src.spawn((Transform::identity(), Name::new("C"))); // created(2), free
    let d = src.spawn((Transform::identity(), Name::new("D"))); // authored, free
                                                                // Created ids are numbered in the order first asked for: A, B, C.
    for e in [a, b, c] {
        src.persistent_id(e).expect("id");
    }
    let a_id = src.mark_authored(a).expect("authored");
    let d_id = src.mark_authored(d).expect("authored");
    let b_id = src.persistent_id(b).expect("id");
    let c_id = src.persistent_id(c).expect("id");
    let record = capture_world(&src).expect("captures");

    let mut dst = World::new();
    let x = dst.spawn((Transform::identity(), Name::new("X"))); // created(0)
    let y = dst.spawn((Transform::identity(), Name::new("Y"))); // created(1)
                                                                // Identified before the load, in spawn order: Y holds B's identity.
    assert!(dst.persistent_id(x).expect("id").is_created());
    assert_eq!(dst.persistent_id(y), Some(b_id));
    let z = dst.spawn((Transform::identity(), Name::new("Z")));
    dst.set_persistent_id(z, a_id);
    let before: Vec<_> = [x, y, z].map(|e| dst.persistent_id(e)).to_vec();

    let applied = apply(&mut dst, &record, Identity::Keep).expect("loads");
    assert_eq!(
        [x, y, z].map(|e| dst.persistent_id(e)).to_vec(),
        before,
        "residents keep theirs"
    );
    let id = |label: &str| {
        let entity = dst
            .iter_entities()
            .find(|e| dst.get::<Name>(*e).map(|n| n.as_str()) == Some(label))
            .expect("loaded");
        dst.persistent_id(entity).expect("has an id")
    };
    assert_eq!(id("C"), c_id, "a free created id is kept");
    assert_eq!(id("D"), d_id, "a free authored id is kept");
    assert!(!id("A").is_created() && id("A") != a_id, "{:?}", id("A"));
    assert!(id("B").is_created() && id("B") != b_id, "{:?}", id("B"));
    let all: std::collections::HashSet<_> = dst
        .iter_entities()
        .map(|e| dst.persistent_id(e).expect("id"))
        .collect();
    assert_eq!(all.len(), 7, "every identity distinct");
    assert_eq!(applied.entities.len(), 4);
    for _ in 0..16 {
        let s = dst.spawn(Transform::identity());
        let sid = dst.persistent_id(s).expect("id");
        assert!(!all.contains(&sid), "a later spawn took {sid:?}");
    }
}

/// Coverage: a created identity at the very top of the namespace loads, and
/// spawns after it keep handing out unique ids without overflow.
#[test]
fn the_largest_created_id_loads_and_spawning_continues() {
    let mut src = World::new();
    let e = src.spawn((Transform::identity(), Name::new("top")));
    src.set_persistent_id(e, PersistentId::from_bits(u64::MAX));
    let record = capture_world(&src).expect("captures");
    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("loads");
    let mut seen = std::collections::HashSet::new();
    seen.insert(PersistentId::from_bits(u64::MAX));
    for _ in 0..64 {
        let s = dst.spawn(Transform::identity());
        let sid = dst.persistent_id(s).expect("id");
        assert!(sid.is_created());
        assert!(seen.insert(sid), "{sid:?} handed out twice");
    }
}

/// Coverage: `capture_subtree` over a `Children` list that loops back to the
/// root and repeats a child (the public API lets one be written) ends, and
/// records each entity once.
#[test]
fn a_subtree_capture_survives_a_looping_children_list() {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("root")));
    let kid = world.spawn((Transform::identity(), Name::new("kid")));
    world
        .add_component(root, Children(vec![kid, kid]))
        .expect("children attach");
    world
        .add_component(kid, Children(vec![root, kid]))
        .expect("children attach");
    let record = khora_data::scene::capture_subtree(&world, root).expect("captures");
    assert_eq!(record.entities.len(), 2, "{:?}", record.entities);
    apply(&mut World::new(), &record, Identity::Fresh).expect("loads");
}

/// Coverage: the cycle check is linear. A 20 000-deep chain plus 20 000
/// leaves hanging off its nodes, listed child-first, loads with the right
/// parent for every node and no false cycle.
#[test]
fn a_long_chain_with_shared_ancestors_loads() {
    const N: usize = 20_000;
    let mut src = World::new();
    let chain: Vec<EntityId> = (0..N)
        .map(|i| src.spawn((Transform::identity(), Name::new(format!("c{i}")))))
        .collect();
    for i in 1..N {
        // Through `add_component`: `set_parent` walks the ancestry each time.
        src.add_component(chain[i], Parent(chain[i - 1]))
            .expect("parent");
    }
    for i in 0..N {
        let leaf = src.spawn((Transform::identity(), Name::new(format!("l{i}"))));
        src.add_component(leaf, Parent(chain[N - 1 - i]))
            .expect("parent");
    }
    let mut record = capture_world(&src).expect("captures");
    record.entities.reverse();
    let start = std::time::Instant::now();
    let mut dst = World::new();
    apply(&mut dst, &record, Identity::Keep).expect("no false cycle");
    let took = start.elapsed();
    assert!(took.as_secs() < 20, "took {took:?}");
    let map = entity_map(&src, &dst);
    for i in 1..N {
        assert_eq!(
            dst.get::<Parent>(map[&chain[i]]).map(|p| p.0),
            Some(map[&chain[i - 1]])
        );
    }
}

/// Coverage: non-finite floats through every encoding: NaN stays NaN, -inf
/// stays -inf, -0.0 keeps its sign; and a `$float` marker in a MessagePack
/// file (a text save repacked by a generic tool) loads too.
#[test]
fn non_finite_floats_round_trip_and_the_marker_reads_in_msgpack() {
    for (z_far, z_near) in [(f32::NAN, -0.0f32), (f32::NEG_INFINITY, 0.1)] {
        let mut src = World::new();
        let eye = src.spawn(Camera::new_perspective(1.0, 1.0, z_near, z_far));
        let id = src.mark_authored(eye).expect("authored");
        for (name, encoding) in every_encoding() {
            let (dst, _) = reload(&src, name, encoding);
            let cam = *dst
                .get::<Camera>(dst.entity_with_id(id).expect("loaded"))
                .expect("camera");
            assert_eq!(cam.z_far.is_nan(), z_far.is_nan(), "{name}");
            if !z_far.is_nan() {
                assert_eq!(cam.z_far, z_far, "{name}");
            }
            assert_eq!(cam.z_near.to_bits(), z_near.to_bits(), "{name}: z_near");
        }
    }

    let mut src = World::new();
    // Every other float exact in an f64: the repacked file holds doubles.
    let eye = src.spawn(Camera::new_perspective(1.0, 1.0, 0.5, f32::INFINITY));
    let id = src.mark_authored(eye).expect("authored");
    let text = TextEncoding
        .encode(&capture_world(&src).expect("captures"))
        .expect("encodes");
    let value: serde_json::Value = serde_json::from_slice(&text).expect("json");
    let packed = rmp_serde::to_vec_named(&value).expect("packs");
    let back = MsgPackEncoding.decode(&packed).expect("decodes");
    let mut dst = World::new();
    apply(&mut dst, &back, Identity::Keep).expect("loads");
    let far = dst
        .get::<Camera>(dst.entity_with_id(id).expect("loaded"))
        .map(|c| c.z_far);
    assert_eq!(far, Some(f32::INFINITY));
}

/// Coverage: a value nested exactly at the record bound is accepted and one
/// level more refused, by compact and by MessagePack (whose own reader
/// allows 1024) alike.
#[test]
fn depth_128_is_accepted_and_129_refused_in_binary_encodings() {
    fn nested(depth: usize) -> Record {
        let mut v = Record::U64(1);
        for _ in 0..depth {
            v = Record::Seq(vec![v]);
        }
        v
    }
    for (depth, ok) in [(128, true), (129, false)] {
        let record = SceneRecord {
            entities: vec![PersistentId::authored(7)],
            pages: vec![PageRecord {
                components: vec!["Transform".into()],
                rows: vec![PersistentId::authored(7)],
                columns: vec![vec![nested(depth)]],
            }],
        };
        for (name, encoding) in [
            ("compact", &CompactEncoding as &dyn SceneEncoding),
            ("msgpack", &MsgPackEncoding),
        ] {
            let bytes = encoding.encode(&record).expect("encodes");
            assert_eq!(encoding.decode(&bytes).is_ok(), ok, "{name} at {depth}");
        }
    }
}
