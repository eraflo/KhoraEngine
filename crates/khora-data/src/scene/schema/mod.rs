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

//! The schema a value is written in, as serde sees it.
//!
//! A snapshot writes values by position, with no names: it can be read only
//! by the schema that wrote it. The schema is traced from a type's serde
//! format — struct and field names, their order, their types, enum variants,
//! nested types — written as canonical text and hashed into a fingerprint
//! that is the same on every run and every build.
//!
//! The canonical text, with no spaces: primitives by name (`bool`, `i8` …
//! `u64`, `f32`, `f64`, `char`, `string`, `bytes`, `unit`); `option<T>`,
//! `seq<T>`, `map<K,V>`, `tuple(T,U)`; `struct Name{a:T}`, `struct Name`,
//! `struct Name(T,U)`; `enum Name{A,B(T),C(T,U),D{x:T}}`, variants in
//! declaration order. A type met again while it is still being written — a
//! recursive type — is written as its name alone.

mod access;
mod trace;

use std::collections::BTreeMap;
use std::fmt;

use serde::de::DeserializeOwned;

/// A container's identity within one trace: its serde name, and the type
/// of the visitor serde reads it with — two types of the same name are two
/// containers. Only the name is ever written.
pub(crate) type Key = (&'static str, &'static str);

/// The format of one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Format {
    /// A primitive, by its canonical name.
    Primitive(&'static str),
    Option(Box<Format>),
    Seq(Box<Format>),
    Map(Box<Format>, Box<Format>),
    Tuple(Vec<Format>),
    /// A struct or an enum: its shape is in the schema's containers.
    Named(Key),
    /// Never reached by the trace.
    Unknown,
}

/// The shape of a named struct or enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Container {
    Unit,
    Tuple(Vec<Format>),
    Struct(Fields),
    /// Each variant, in declaration order, and its payload once traced.
    Enum(Vec<(&'static str, Option<Payload>)>),
}

/// A struct's fields: the names serde knows it by — each field's, and the
/// older names it is also read under — and the format of each field, in
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fields {
    pub(crate) names: &'static [&'static str],
    pub(crate) formats: Vec<Format>,
}

/// What an enum variant carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Payload {
    Unit,
    Tuple(Vec<Format>),
    Struct(Fields),
}

/// The serde format of a type: a canonical tree whose `Display` is its
/// canonical text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    root: Format,
    containers: BTreeMap<Key, Container>,
    /// Whether every value of the type was traced. A type whose trace could
    /// not finish is described only in part, and says so in its text.
    complete: bool,
}

impl Schema {
    /// The 64-bit fingerprint of the canonical text (FNV-1a).
    pub fn fingerprint(&self) -> u64 {
        fingerprint_of(&self.to_string())
    }

    /// Whether every value of the type was traced.
    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

/// The 64-bit FNV-1a of `text`: how a canonical text becomes a fingerprint,
/// for a schema and for anything built from several.
pub fn fingerprint_of(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl fmt::Display for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_format(f, &self.root, &mut Vec::new())?;
        if !self.complete {
            f.write_str("!incomplete")?;
        }
        Ok(())
    }
}

impl Schema {
    fn write_format(
        &self,
        f: &mut fmt::Formatter<'_>,
        format: &Format,
        open: &mut Vec<Key>,
    ) -> fmt::Result {
        match format {
            Format::Primitive(name) => f.write_str(name),
            Format::Option(inner) => {
                f.write_str("option<")?;
                self.write_format(f, inner, open)?;
                f.write_str(">")
            }
            Format::Seq(inner) => {
                f.write_str("seq<")?;
                self.write_format(f, inner, open)?;
                f.write_str(">")
            }
            Format::Map(key, value) => {
                f.write_str("map<")?;
                self.write_format(f, key, open)?;
                f.write_str(",")?;
                self.write_format(f, value, open)?;
                f.write_str(">")
            }
            Format::Tuple(items) => {
                f.write_str("tuple")?;
                self.write_list(f, items, open)
            }
            Format::Unknown => f.write_str("?"),
            Format::Named(key) => {
                let Some(container) = self.containers.get(key) else {
                    return f.write_str(key.0);
                };
                if open.contains(key) {
                    return f.write_str(key.0);
                }
                open.push(*key);
                let written = self.write_container(f, key.0, container, open);
                open.pop();
                written
            }
        }
    }

    fn write_container(
        &self,
        f: &mut fmt::Formatter<'_>,
        name: &str,
        container: &Container,
        open: &mut Vec<Key>,
    ) -> fmt::Result {
        match container {
            Container::Unit => write!(f, "struct {name}"),
            Container::Tuple(items) => {
                write!(f, "struct {name}")?;
                self.write_list(f, items, open)
            }
            Container::Struct(fields) => {
                write!(f, "struct {name}")?;
                self.write_fields(f, fields, open)
            }
            Container::Enum(variants) => {
                write!(f, "enum {name}{{")?;
                for (index, (variant, payload)) in variants.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(variant)?;
                    match payload {
                        Some(Payload::Unit) => {}
                        Some(Payload::Tuple(items)) => self.write_list(f, items, open)?,
                        Some(Payload::Struct(fields)) => self.write_fields(f, fields, open)?,
                        None => f.write_str("?")?,
                    }
                }
                f.write_str("}")
            }
        }
    }

    fn write_list(
        &self,
        f: &mut fmt::Formatter<'_>,
        items: &[Format],
        open: &mut Vec<Key>,
    ) -> fmt::Result {
        f.write_str("(")?;
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            self.write_format(f, item, open)?;
        }
        f.write_str(")")
    }

    /// `{a:T,b:U}` — or, for a struct read under older names too, which
    /// serde lists beside the current ones, every name then the formats:
    /// `{reserve,points,regen}(f32,f32)`.
    fn write_fields(
        &self,
        f: &mut fmt::Formatter<'_>,
        fields: &Fields,
        open: &mut Vec<Key>,
    ) -> fmt::Result {
        if fields.names.len() == fields.formats.len() {
            f.write_str("{")?;
            for (index, (field, format)) in fields.names.iter().zip(&fields.formats).enumerate() {
                if index > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{field}:")?;
                self.write_format(f, format, open)?;
            }
            return f.write_str("}");
        }
        write!(f, "{{{}}}", fields.names.join(","))?;
        self.write_list(f, &fields.formats, open)
    }
}

/// `T`'s fingerprint and whether its trace was complete, traced once into
/// `cell`. An incomplete trace is said out loud: a snapshot of `name` cannot
/// be guarded, so none is written or read.
pub fn traced_once<T: DeserializeOwned>(
    cell: &std::sync::OnceLock<(u64, bool)>,
    name: &str,
) -> (u64, bool) {
    *cell.get_or_init(|| {
        let schema = schema_of::<T>();
        if !schema.is_complete() {
            log::warn!(
                "the schema of `{name}` could not be traced in full ({schema}): \
                 it is never snapshotted"
            );
        }
        (schema.fingerprint(), schema.is_complete())
    })
}

/// Traces the serde format of `T`.
///
/// Drives `T::deserialize` with a deserializer that records every format it
/// is asked for, as many times as it takes to visit every variant of every
/// enum. Recursion ends where serde lets a value stop: an empty sequence or
/// map, a `None`, an enum variant already known to end.
pub fn schema_of<T: DeserializeOwned>() -> Schema {
    let (root, containers, complete) = trace::trace::<T>();
    Schema {
        root,
        containers,
        complete,
    }
}

#[cfg(test)]
mod tests;
