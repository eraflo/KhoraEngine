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

//! A component whose schema no fingerprint can guard is never snapshotted.
//!
//! Its own test binary: the component registers itself, and in the
//! `scene_records` binary it would join every registration those tests
//! iterate. `#[derive(Component)]` spells its output with `crate::ecs` /
//! `crate::scene` paths, re-exported below.

use khora_core::scene::SceneFile;
use khora_data::ecs::{Name, World};
use khora_data::scene::snapshot::{prepare_snapshot, write_snapshot};
use khora_data::scene::{
    apply, capture_world, read_scene_file, write_scene_file, CompactEncoding,
    ComponentRegistration, Identity, SaveError,
};
use khora_macros::Component;
use serde::{Deserialize, Deserializer, Serialize};

/// The `crate::ecs` the derive expands against.
mod ecs {
    pub use khora_data::ecs::*;
}

/// The `crate::scene` the derive expands against.
mod scene {
    pub use khora_data::scene::*;
}

/// A value read from a string none of the tracer's placeholders (`""`,
/// `"a"`, `"0"`) is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Version(String);

impl Default for Version {
    fn default() -> Self {
        Self("0.0.0".into())
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.split('.').count() == 3 {
            Ok(Self(text))
        } else {
            Err(serde::de::Error::custom("not a version"))
        }
    }
}

/// A component holding one: its trace stops at `version`.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct Versioned {
    pub version: Version,
    pub scale: f32,
}

fn registration() -> &'static ComponentRegistration {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .find(|reg| reg.type_name == "Versioned")
        .expect("Versioned registers itself")
}

fn versioned() -> Versioned {
    Versioned {
        version: Version("1.2.3".into()),
        scale: 2.5,
    }
}

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

/// The registration says its schema is incomplete.
#[test]
fn the_registration_knows_its_schema_is_incomplete() {
    assert!(!(registration().schema_complete)());
}

/// `write_snapshot` refuses a world holding it, naming it — and a world
/// without it is still snapshotted.
#[test]
fn a_snapshot_of_an_unguarded_component_is_refused() {
    let mut world = World::new();
    world.spawn(Name::new("plain"));
    write_snapshot(&world).expect("a world without it is snapshotted");
    world.spawn(versioned());
    match write_snapshot(&world) {
        Err(SaveError::Unguarded(name)) => assert_eq!(name, "Versioned"),
        other => panic!("not refused as unguarded: {other:?}"),
    }
}

/// A snapshot naming it — written by hand, under its current fingerprint —
/// is refused, naming it, the world unchanged.
#[test]
fn a_snapshot_holding_an_unguarded_component_is_not_loaded() {
    let mut src = World::new();
    let entity = src.spawn(Name::new("x"));
    let id = src.persistent_id(entity).expect("an id");

    let mut payload = vec![1];
    varint(&mut payload, 1);
    varint(&mut payload, "Versioned".len() as u64);
    payload.extend_from_slice(b"Versioned");
    payload.extend_from_slice(&(registration().schema)().to_le_bytes());
    varint(&mut payload, 1);
    payload.extend_from_slice(&id.to_bits().to_le_bytes());
    varint(&mut payload, 1); // one page
    varint(&mut payload, 1); // one column
    varint(&mut payload, 0);
    varint(&mut payload, 1); // one row
    payload.extend_from_slice(&id.to_bits().to_le_bytes());
    let mut value = vec![5];
    value.extend_from_slice(b"1.2.3");
    value.extend_from_slice(&2.5f32.to_le_bytes());
    varint(&mut payload, value.len() as u64);
    payload.extend_from_slice(&value);

    let mut file: SceneFile = write_snapshot(&World::new()).expect("written");
    file.header.payload_length = payload.len() as u64;
    file.payload = payload;

    let mut dst = World::new();
    dst.spawn(Name::new("already here"));
    let before = dst.iter_entities().count();
    let refused = prepare_snapshot(&mut dst, &file)
        .err()
        .expect("an unguarded component is refused");
    assert!(refused.message.contains("Versioned"), "{}", refused.message);
    assert_eq!(dst.iter_entities().count(), before);
}

/// What `FastestLoad` falls back to for such a world — a compact record —
/// brings the component back.
#[test]
fn the_compact_fallback_loads_it_back() {
    let mut src = World::new();
    src.spawn(versioned());
    let record = capture_world(&src).expect("captured");
    let file = write_scene_file(&record, &CompactEncoding).expect("encoded");
    let file = SceneFile::from_bytes(&file.to_bytes()).expect("parses");
    let back = read_scene_file(&file).expect("decoded");
    let mut dst = World::new();
    let applied = apply(&mut dst, &back, Identity::Keep).expect("loaded");
    let (_, entity) = applied.entities[0];
    assert_eq!(dst.get::<Versioned>(entity), Some(&versioned()));
}
