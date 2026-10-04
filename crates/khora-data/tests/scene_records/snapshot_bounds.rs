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

//! The snapshot encoding's bounds: the tracer's retries and slots,
//! the positional depth and count bounds.

#![allow(dead_code)]

use std::collections::BTreeMap;

use khora_data::scene::positional::{from_positional, to_positional};
use khora_data::scene::record::{EntityRef, RecordError, ReferenceReader, ReferenceWriter};
use khora_data::scene::schema::schema_of;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

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

// --- A slot shared by a field and an element inside another field --------

mod tuple_u8 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub next: Option<Box<Node>>,
        pub limits: (Option<u8>, u8),
    }
}
mod tuple_u64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub next: Option<Box<Node>>,
        pub limits: (Option<u64>, u8),
    }
}

/// A tuple's elements take the slots of the struct holding it:
/// element 0 of `limits` is the slot of field 0, `next`, which recursed and
/// is answered `None` with *its* inner format — so `limits[0]` is written as
/// `option<Node>` and its real type never reaches the fingerprint.
#[test]
fn a_tuple_element_is_not_answered_with_another_fields_none() {
    let text = schema_of::<tuple_u8::Node>().to_string();
    assert!(text.contains("option<u8>"), "`{text}`");
    assert_ne!(
        fingerprint::<tuple_u8::Node>(),
        fingerprint::<tuple_u64::Node>(),
        "`{text}`"
    );
}

// --- A value no placeholder satisfies -------------------------------------

/// A value read from a string that none of the tracer's placeholders
/// (`""`, `"a"`, `"0"`) is — a version, a URL, a parsed name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Version(String);

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

mod version_f32 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub version: Version,
        pub scale: f32,
    }
}
mod version_f64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub version: Version,
        pub scale: f64,
    }
}

/// serde cannot read past a field that refuses every placeholder, so the
/// trace of such a type is incomplete and its fingerprint guards nothing
/// after the refusal. It says so — and the system refuses to snapshot it:
/// `write_snapshot` / `prepare_snapshot` refuse the component, and
/// `FastestLoad` falls back to a compact record. Those checks need a
/// registered component holding a `Version`, which would join every
/// registration this binary's other tests iterate: they live in their own
/// binary, `tests/snapshot_unguarded.rs`.
#[test]
fn an_incomplete_schema_says_so() {
    let schema = schema_of::<version_f32::Probe>();
    assert!(!schema.is_complete(), "`{schema}`");
    assert!(!schema_of::<version_f64::Probe>().is_complete());
}

// --- Writer and reader agree at MAX_DEPTH -------------------------------

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum EnumNewtype {
    End,
    Next(Box<EnumNewtype>),
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum EnumTuple {
    End,
    Next(u8, Box<EnumTuple>),
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum EnumStruct {
    End,
    Next { v: u8, next: Box<EnumStruct> },
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OptionChain(Option<Box<OptionChain>>);
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct StructChain {
    next: Option<Box<StructChain>>,
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct SeqChain(Vec<SeqChain>);
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct MapChain(BTreeMap<u8, MapChain>);
#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum TupleChain {
    End,
    Next((u8, Box<TupleChain>)),
}

/// The deepest value of a kind the writer accepts, read back.
fn deepest_reads_back<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
    kind: &str,
    build: impl Fn(usize) -> T,
) {
    let mut deepest = None;
    for links in 0..400 {
        let mut bytes = Vec::new();
        if to_positional(&build(links), &mut bytes, &mut NoRefs).is_err() {
            break;
        }
        deepest = Some((links, bytes));
    }
    let (links, bytes) = deepest.expect("something is written");
    assert!(links < 399, "{kind}: the writer never refused");
    let back = from_positional::<T>(&bytes, &mut NoRefs);
    assert!(
        back.as_ref().ok() == Some(&build(links)),
        "{kind}: {links} levels written, read back as {:?}",
        back.err()
    );
}

#[test]
fn what_the_writer_accepts_at_max_depth_the_reader_reads() {
    deepest_reads_back("enum newtype", |n| {
        (0..n).fold(EnumNewtype::End, |next, _| {
            EnumNewtype::Next(Box::new(next))
        })
    });
    deepest_reads_back("enum tuple", |n| {
        (0..n).fold(EnumTuple::End, |next, _| EnumTuple::Next(1, Box::new(next)))
    });
    deepest_reads_back("enum struct", |n| {
        (0..n).fold(EnumStruct::End, |next, _| EnumStruct::Next {
            v: 1,
            next: Box::new(next),
        })
    });
    deepest_reads_back("option", |n| {
        (0..n).fold(OptionChain(None), |next, _| {
            OptionChain(Some(Box::new(next)))
        })
    });
    deepest_reads_back("struct", |n| {
        (0..n).fold(StructChain { next: None }, |next, _| StructChain {
            next: Some(Box::new(next)),
        })
    });
    deepest_reads_back("seq", |n| {
        (0..n).fold(SeqChain(vec![]), |next, _| SeqChain(vec![next]))
    });
    deepest_reads_back("map", |n| {
        (0..n).fold(MapChain(BTreeMap::new()), |next, _| {
            MapChain(BTreeMap::from([(1, next)]))
        })
    });
    deepest_reads_back("tuple", |n| {
        (0..n).fold(TupleChain::End, |next, _| {
            TupleChain::Next((1, Box::new(next)))
        })
    });
}

// --- Elements that take no bytes, past the slack ----------------------

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
struct Marker;

/// Whatever the writer writes, the reader reads: a sequence of zero-size
/// elements past the reader's slack is refused on write, or its bytes read
/// back — and one within the slack (256) round-trips.
#[test]
fn every_sequence_of_unit_elements_written_reads_back() {
    for count in [256, 257, 300, 10_000] {
        let value = vec![Marker; count];
        let mut bytes = Vec::new();
        if to_positional(&value, &mut bytes, &mut NoRefs).is_ok() {
            assert_eq!(
                from_positional::<Vec<Marker>>(&bytes, &mut NoRefs).as_ref(),
                Ok(&value),
                "{count} units written, not read back"
            );
        } else {
            assert!(
                bytes.is_empty(),
                "{count}: a refused write left bytes behind"
            );
        }
    }
    let value = vec![Marker; 256];
    let mut bytes = Vec::new();
    to_positional(&value, &mut bytes, &mut NoRefs).expect("256 units are written");
    assert_eq!(
        from_positional::<Vec<Marker>>(&bytes, &mut NoRefs),
        Ok(value)
    );
}

// --- A recursion cut short keeps a partial format ------------------------

mod weighted_u8 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub parent: Option<(Box<Node>, u8)>,
        pub children: Vec<(Node, u8)>,
        pub named: BTreeMap<String, (Node, u8)>,
    }
}
mod weighted_u64 {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub parent: Option<(Box<Node>, u64)>,
        pub children: Vec<(Node, u64)>,
        pub named: BTreeMap<String, (Node, u64)>,
    }
}

/// Where a recursion is cut — an option answered `None`, a sequence or a
/// map answered empty — the format kept is the one the failed run saw, and
/// that run stopped at the recursion: a `(Node, u8)` is written `tuple(Node)`,
/// its `u8` never reaching the fingerprint.
#[test]
fn a_cut_recursion_keeps_the_whole_format_of_what_holds_it() {
    let text = schema_of::<weighted_u8::Node>().to_string();
    assert!(text.contains("tuple(Node,u8)"), "`{text}`");
    assert_ne!(
        fingerprint::<weighted_u8::Node>(),
        fingerprint::<weighted_u64::Node>(),
        "`{text}`"
    );
}
