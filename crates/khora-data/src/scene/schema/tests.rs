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

//! Tests of the schema tracer.
//!
//! Each change a snapshot must not survive — a field renamed, retyped,
//! reordered or added, a variant gained or renamed, a nested type changed —
//! is a pair of types with the same serde names but for that one change,
//! declared in sibling modules. Their fingerprints must differ.
//!
//! # The canonical text
//!
//! The text a [`Schema`] displays is the contract a fingerprint hashes, so it
//! is pinned here, in this grammar (no spaces):
//!
//! - primitives by name: `bool`, `i8` … `i64`, `u8` … `u64`, `f32`, `f64`,
//!   `char`, `string`, `bytes`, `unit`;
//! - `option<T>`, `seq<T>`, `map<K,V>`, `tuple(T,U,…)`;
//! - a struct `struct Name{a:T,b:U}`, a unit struct `struct Name`, a newtype
//!   or tuple struct `struct Name(T,U)`;
//! - an enum `enum Name{A,B(T),C(T,U),D{x:T}}`, variants in declaration order;
//! - a type met again while it is still being traced — a recursive type —
//!   written as its name alone.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use super::*;
use crate::scene::ComponentRegistration;

/// FNV-1a, 64 bits — what the fingerprint of a canonical text is.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The fingerprint of `T`.
fn fingerprint<T: DeserializeOwned>() -> u64 {
    schema_of::<T>().fingerprint()
}

/// Asserts `A` and `B` — the same type but for `change` — trace to different
/// schemas and fingerprints.
fn assert_differ<A: DeserializeOwned, B: DeserializeOwned>(change: &str) {
    let (a, b) = (schema_of::<A>(), schema_of::<B>());
    assert_ne!(a, b, "{change}: the schemas are equal");
    assert_ne!(
        a.to_string(),
        b.to_string(),
        "{change}: the canonical texts are equal"
    );
    assert_ne!(
        a.fingerprint(),
        b.fingerprint(),
        "{change}: the fingerprints collide ({a} / {b})"
    );
}

#[derive(Deserialize)]
struct Pinned {
    id: u32,
    label: String,
    scale: Option<f32>,
    tags: Vec<bool>,
    weights: BTreeMap<String, f64>,
}

/// The canonical text of a small struct is fixed — names, order and types,
/// and nothing a build or a run could change (no `TypeId`, no address, no
/// Rust path) — and its fingerprint is that text's FNV-1a.
#[test]
fn the_canonical_text_is_build_independent() {
    let schema = schema_of::<Pinned>();
    let text = schema.to_string();
    assert_eq!(
        text,
        "struct Pinned{id:u32,label:string,scale:option<f32>,tags:seq<bool>,\
         weights:map<string,f64>}"
    );
    assert_eq!(schema.fingerprint(), fnv1a_64(text.as_bytes()));
}

/// Tracing a type twice gives the same schema and the same fingerprint.
#[test]
fn a_fingerprint_is_stable() {
    assert_eq!(schema_of::<Pinned>(), schema_of::<Pinned>());
    assert_eq!(fingerprint::<Pinned>(), fingerprint::<Pinned>());
    let schema = schema_of::<Pinned>();
    assert_eq!(schema.fingerprint(), schema.fingerprint());
    assert_eq!(
        schema.fingerprint(),
        fnv1a_64(schema.to_string().as_bytes()),
        "the fingerprint hashes the canonical text"
    );
}

mod base {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub a: u32,
        pub b: f32,
    }
}
mod field_renamed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub a: u32,
        pub c: f32,
    }
}
mod field_retyped {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub a: u32,
        pub b: f64,
    }
}
mod field_reordered {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub b: f32,
        pub a: u32,
    }
}
mod field_added {
    use super::*;
    #[derive(Deserialize)]
    pub struct Probe {
        pub a: u32,
        pub b: f32,
        pub c: bool,
    }
}
mod type_renamed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Sonde {
        pub a: u32,
        pub b: f32,
    }
}

/// A field renamed, retyped, reordered or added — or the struct renamed —
/// is a different schema.
#[test]
fn a_changed_field_changes_the_fingerprint() {
    assert_differ::<base::Probe, field_renamed::Probe>("a field renamed");
    assert_differ::<base::Probe, field_retyped::Probe>("a field retyped");
    assert_differ::<base::Probe, field_reordered::Probe>("fields reordered");
    assert_differ::<base::Probe, field_added::Probe>("a field added");
    assert_differ::<base::Probe, type_renamed::Sonde>("the struct renamed");
}

mod enum_base {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Off,
        Level(u8),
        Span(f32, f32),
        Area { w: f32, h: f32 },
    }
}
mod enum_gained {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Off,
        Level(u8),
        Span(f32, f32),
        Area { w: f32, h: f32 },
        Burst,
    }
}
mod enum_variant_renamed {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Idle,
        Level(u8),
        Span(f32, f32),
        Area { w: f32, h: f32 },
    }
}
mod enum_first_payload {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Off,
        Level(u16),
        Span(f32, f32),
        Area { w: f32, h: f32 },
    }
}
mod enum_last_payload {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Off,
        Level(u8),
        Span(f32, f32),
        Area { w: f32, h: f64 },
    }
}
mod enum_variant_kind {
    use super::*;
    #[derive(Deserialize)]
    pub enum Mode {
        Off(u8),
        Level(u8),
        Span(f32, f32),
        Area { w: f32, h: f32 },
    }
}

/// An enum that gains or renames a variant, or whose variant carries
/// something else — the last variant included, so the tracer must visit
/// every variant, not only the first — is a different schema.
#[test]
fn a_changed_variant_changes_the_fingerprint() {
    assert_differ::<enum_base::Mode, enum_gained::Mode>("a variant gained");
    assert_differ::<enum_base::Mode, enum_variant_renamed::Mode>("a variant renamed");
    assert_differ::<enum_base::Mode, enum_first_payload::Mode>("a newtype payload retyped");
    assert_differ::<enum_base::Mode, enum_last_payload::Mode>("the last variant's field retyped");
    assert_differ::<enum_base::Mode, enum_variant_kind::Mode>("a unit variant became newtype");
}

/// The canonical text of an enum names every variant, in order, with what it
/// carries.
#[test]
fn an_enum_traces_every_variant() {
    assert_eq!(
        schema_of::<enum_base::Mode>().to_string(),
        "enum Mode{Off,Level(u8),Span(f32,f32),Area{w:f32,h:f32}}"
    );
}

mod nested_base {
    use super::*;
    #[derive(Deserialize)]
    pub struct Inner {
        pub x: f32,
    }
    #[derive(Deserialize)]
    pub struct Outer {
        pub inner: Inner,
        pub maybe: Option<Inner>,
        pub many: Vec<Inner>,
        pub keyed: BTreeMap<String, Inner>,
    }
    #[derive(Deserialize)]
    pub struct Wrapped {
        pub opt: Option<u32>,
        pub list: Vec<u32>,
        pub table: HashMap<u32, u32>,
    }
}
mod nested_changed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Inner {
        pub x: f64,
    }
    #[derive(Deserialize)]
    pub struct Outer {
        pub inner: Inner,
        pub maybe: Option<Inner>,
        pub many: Vec<Inner>,
        pub keyed: BTreeMap<String, Inner>,
    }
}
mod option_changed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Wrapped {
        pub opt: Option<i32>,
        pub list: Vec<u32>,
        pub table: HashMap<u32, u32>,
    }
}
mod seq_changed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Wrapped {
        pub opt: Option<u32>,
        pub list: Vec<u64>,
        pub table: HashMap<u32, u32>,
    }
}
mod map_changed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Wrapped {
        pub opt: Option<u32>,
        pub list: Vec<u32>,
        pub table: HashMap<u32, String>,
    }
}

/// A nested struct's field, the `T` of an `Option<T>`, of a `Vec<T>`, the
/// value type of a map: each is part of the schema.
#[test]
fn a_changed_inner_type_changes_the_fingerprint() {
    assert_differ::<nested_base::Outer, nested_changed::Outer>("a nested struct's field");
    assert_differ::<nested_base::Wrapped, option_changed::Wrapped>("an option's inner type");
    assert_differ::<nested_base::Wrapped, seq_changed::Wrapped>("a vec's element type");
    assert_differ::<nested_base::Wrapped, map_changed::Wrapped>("a map's value type");
}

/// Only the inner type of an option tells two options apart: the option
/// itself is traced as present, not left as an opaque `option`.
#[test]
fn an_option_and_a_sequence_trace_their_inner_types() {
    let text = schema_of::<nested_base::Wrapped>().to_string();
    assert_eq!(
        text,
        "struct Wrapped{opt:option<u32>,list:seq<u32>,table:map<u32,u32>}"
    );
}

mod recursive {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub value: u32,
        pub children: Vec<Node>,
    }
    #[derive(Deserialize)]
    pub enum Tree {
        Leaf(u32),
        Branch(Box<Tree>, Box<Tree>),
    }
    #[derive(Deserialize)]
    pub struct List {
        pub value: u8,
        pub next: Option<Box<List>>,
    }
    #[derive(Deserialize)]
    pub struct Ping {
        pub pong: Option<Box<Pong>>,
    }
    #[derive(Deserialize)]
    pub struct Pong {
        pub pings: Vec<Ping>,
    }
}
mod recursive_changed {
    use super::*;
    #[derive(Deserialize)]
    pub struct Node {
        pub value: u64,
        pub children: Vec<Node>,
    }
}

/// A type that contains itself — through a vec, a box, an option, or another
/// type — traces in finite time, and still tells a changed leaf apart.
#[test]
fn a_recursive_type_terminates() {
    assert_eq!(
        schema_of::<recursive::Node>().to_string(),
        "struct Node{value:u32,children:seq<Node>}"
    );
    let tree = schema_of::<recursive::Tree>().to_string();
    assert!(tree.starts_with("enum Tree{"), "{tree}");
    let list = schema_of::<recursive::List>().to_string();
    assert!(list.contains("option<List>"), "{list}");
    let ping = schema_of::<recursive::Ping>().to_string();
    assert!(ping.contains("Pong"), "{ping}");
    assert_eq!(
        fingerprint::<recursive::Node>(),
        fingerprint::<recursive::Node>()
    );
    assert_differ::<recursive::Node, recursive_changed::Node>("a recursive type's leaf");
}

/// Every registered component traces — its registration's fingerprint is
/// computed without panicking, the same on every call — and no two
/// components share one.
#[test]
fn every_registration_has_its_own_fingerprint() {
    let mut seen: HashMap<u64, &'static str> = HashMap::new();
    let mut count = 0;
    for reg in inventory::iter::<ComponentRegistration> {
        let first = (reg.schema)();
        assert_eq!(first, (reg.schema)(), "{}: not stable", reg.type_name);
        if let Some(other) = seen.insert(first, reg.type_name) {
            panic!(
                "`{}` and `{other}` share the fingerprint {first:#x}",
                reg.type_name
            );
        }
        count += 1;
    }
    assert!(count >= 30, "only {count} registrations were traced");
}

/// A derived registration's fingerprint is that of the mirror a snapshot
/// writes the component through.
#[test]
fn a_registration_fingerprints_its_mirror() {
    use crate::ecs::{SerializableName, SerializableTransform};
    use crate::scene::registration_of;

    let transform = registration_of("Transform").expect("Transform is registered");
    assert_eq!((transform.schema)(), fingerprint::<SerializableTransform>());
    let name = registration_of("Name").expect("Name is registered");
    assert_eq!((name.schema)(), fingerprint::<SerializableName>());
}
