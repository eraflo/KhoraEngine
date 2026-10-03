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

//! The compact encoding: every name once, values by position.
//!
//! A scene repeats the same few shapes thousands of times — a `Transform`
//! per entity, each with the same three field names. So the file starts with
//! a table of every name it uses and a table of every shape (a struct's name
//! and field names, an enum's name and a variant's), and each value after
//! that refers to its shape by number and lists its fields in order. Names
//! are kept — that is what lets a reader match them against today's code —
//! but each is written once.
//!
//! Layout: `version: u8`, the symbol table, the shape table, the entities,
//! then the pages — each its signature, its rows, and its values column by
//! column, the way CRPECS stores them. Counts and integers are LEB128 varints
//! (signed ones zig-zagged); persistent ids and floats are little-endian at
//! their width. Every count is checked against the bytes left before anything
//! is allocated for it: a count is the file's claim, not a fact.

mod reader;

use std::cell::Cell;
use std::collections::HashMap;

use reader::{Reader, NAME_BYTES_PER_FILE_BYTE};

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;

use super::EncodingError;
use crate::scene::record::{EntityRef, Record, VariantPayload};
use crate::scene::save::{RemovedComponents, SaveRecord};
use crate::scene::scene_record::{PageRecord, SceneRecord};

/// The layout version this module writes and reads.
const VERSION: u8 = 1;

mod tag {
    pub const UNIT: u8 = 0;
    pub const FALSE: u8 = 1;
    pub const TRUE: u8 = 2;
    pub const I64: u8 = 3;
    pub const U64: u8 = 4;
    pub const F32: u8 = 5;
    pub const F64: u8 = 6;
    pub const CHAR: u8 = 7;
    pub const STR: u8 = 8;
    pub const BYTES: u8 = 9;
    pub const NONE: u8 = 10;
    pub const SOME: u8 = 11;
    pub const SEQ: u8 = 12;
    pub const MAP: u8 = 13;
    pub const UNIT_STRUCT: u8 = 14;
    pub const STRUCT: u8 = 15;
    pub const TUPLE_STRUCT: u8 = 16;
    pub const NEWTYPE: u8 = 17;
    pub const VARIANT: u8 = 18;
    pub const ENTITY: u8 = 19;
    pub const OUTSIDE: u8 = 20;
    pub const ASSET: u8 = 21;
    pub const DECIMAL: u8 = 22;

    pub const PAYLOAD_UNIT: u8 = 0;
    pub const PAYLOAD_NEWTYPE: u8 = 1;
    pub const PAYLOAD_TUPLE: u8 = 2;
    pub const PAYLOAD_STRUCT: u8 = 3;
}

/// A struct's or a variant's shape: what its values do not repeat.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Shape {
    /// A struct: its name and its field names, in order.
    Struct { name: u32, fields: Vec<u32> },
    /// An enum variant: the enum's name and the variant's.
    Variant { enum_name: u32, variant: u32 },
}

/// Writes `record` compactly.
pub(super) fn encode(record: &SceneRecord) -> Vec<u8> {
    let mut tables = Tables::default();
    let mut body = Vec::new();
    write_varint(&mut body, record.entities.len() as u64);
    for id in &record.entities {
        body.extend_from_slice(&id.to_bits().to_le_bytes());
    }
    write_varint(&mut body, record.pages.len() as u64);
    for page in &record.pages {
        write_varint(&mut body, page.components.len() as u64);
        for name in &page.components {
            let name = tables.symbol(name);
            write_varint(&mut body, name.into());
        }
        write_varint(&mut body, page.rows.len() as u64);
        for id in &page.rows {
            body.extend_from_slice(&id.to_bits().to_le_bytes());
        }
        // Column by column, as the page stores them.
        for column in &page.columns {
            for value in column {
                tables.write_value(&mut body, value);
            }
        }
    }

    let mut out = vec![VERSION];
    write_varint(&mut out, tables.symbols.len() as u64);
    for symbol in &tables.symbols {
        write_varint(&mut out, symbol.len() as u64);
        out.extend_from_slice(symbol.as_bytes());
    }
    write_varint(&mut out, tables.shapes.len() as u64);
    for shape in &tables.shapes {
        match shape {
            Shape::Struct { name, fields } => {
                out.push(0);
                write_varint(&mut out, (*name).into());
                write_varint(&mut out, fields.len() as u64);
                for field in fields {
                    write_varint(&mut out, (*field).into());
                }
            }
            Shape::Variant { enum_name, variant } => {
                out.push(1);
                write_varint(&mut out, (*enum_name).into());
                write_varint(&mut out, (*variant).into());
            }
        }
    }
    out.extend_from_slice(&body);
    out
}

/// The name and shape tables a file is being written with.
#[derive(Default)]
struct Tables {
    symbols: Vec<String>,
    symbol_ids: HashMap<String, u32>,
    shapes: Vec<Shape>,
    shape_ids: HashMap<Shape, u32>,
}

impl Tables {
    fn symbol(&mut self, name: &str) -> u32 {
        if let Some(id) = self.symbol_ids.get(name) {
            return *id;
        }
        let id = self.symbols.len() as u32;
        self.symbols.push(name.to_owned());
        self.symbol_ids.insert(name.to_owned(), id);
        id
    }

    fn shape(&mut self, shape: Shape) -> u32 {
        if let Some(id) = self.shape_ids.get(&shape) {
            return *id;
        }
        let id = self.shapes.len() as u32;
        self.shapes.push(shape.clone());
        self.shape_ids.insert(shape, id);
        id
    }

    fn struct_shape(&mut self, name: &str, fields: &[(String, Record)]) -> u32 {
        let name = self.symbol(name);
        let fields = fields.iter().map(|(field, _)| self.symbol(field)).collect();
        self.shape(Shape::Struct { name, fields })
    }

    fn write_value(&mut self, out: &mut Vec<u8>, value: &Record) {
        match value {
            Record::Unit => out.push(tag::UNIT),
            Record::Bool(false) => out.push(tag::FALSE),
            Record::Bool(true) => out.push(tag::TRUE),
            Record::I64(v) => {
                out.push(tag::I64);
                write_varint(out, zigzag(*v));
            }
            Record::U64(v) => {
                out.push(tag::U64);
                write_varint(out, *v);
            }
            Record::F32(v) => {
                out.push(tag::F32);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Record::F64(v) => {
                out.push(tag::F64);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Record::Decimal(v) => {
                out.push(tag::DECIMAL);
                out.extend_from_slice(&v.to_le_bytes());
            }
            Record::Char(v) => {
                out.push(tag::CHAR);
                write_varint(out, u64::from(*v));
            }
            Record::Str(v) => {
                out.push(tag::STR);
                write_varint(out, v.len() as u64);
                out.extend_from_slice(v.as_bytes());
            }
            Record::Bytes(v) => {
                out.push(tag::BYTES);
                write_varint(out, v.len() as u64);
                out.extend_from_slice(v);
            }
            Record::None => out.push(tag::NONE),
            Record::Some(inner) => {
                out.push(tag::SOME);
                self.write_value(out, inner);
            }
            Record::Seq(items) => {
                out.push(tag::SEQ);
                self.write_items(out, items);
            }
            Record::Map(entries) => {
                out.push(tag::MAP);
                write_varint(out, entries.len() as u64);
                for (key, value) in entries {
                    self.write_value(out, key);
                    self.write_value(out, value);
                }
            }
            Record::UnitStruct { name } => {
                out.push(tag::UNIT_STRUCT);
                let name = self.symbol(name);
                write_varint(out, name.into());
            }
            Record::Struct { name, fields } => {
                out.push(tag::STRUCT);
                let shape = self.struct_shape(name, fields);
                write_varint(out, shape.into());
                for (_, value) in fields {
                    self.write_value(out, value);
                }
            }
            Record::TupleStruct { name, fields } => {
                out.push(tag::TUPLE_STRUCT);
                let name = self.symbol(name);
                write_varint(out, name.into());
                self.write_items(out, fields);
            }
            Record::Newtype { name, value } => {
                out.push(tag::NEWTYPE);
                let name = self.symbol(name);
                write_varint(out, name.into());
                self.write_value(out, value);
            }
            Record::Variant {
                enum_name,
                variant,
                payload,
            } => {
                out.push(tag::VARIANT);
                let enum_name = self.symbol(enum_name);
                let variant = self.symbol(variant);
                let shape = self.shape(Shape::Variant { enum_name, variant });
                write_varint(out, shape.into());
                match payload {
                    VariantPayload::Unit => out.push(tag::PAYLOAD_UNIT),
                    VariantPayload::Newtype(inner) => {
                        out.push(tag::PAYLOAD_NEWTYPE);
                        self.write_value(out, inner);
                    }
                    VariantPayload::Tuple(items) => {
                        out.push(tag::PAYLOAD_TUPLE);
                        self.write_items(out, items);
                    }
                    VariantPayload::Struct(fields) => {
                        out.push(tag::PAYLOAD_STRUCT);
                        // The fields' shape, named after nothing: the
                        // variant's own shape already names it.
                        let fields_shape = self.struct_shape("", fields);
                        write_varint(out, fields_shape.into());
                        for (_, value) in fields {
                            self.write_value(out, value);
                        }
                    }
                }
            }
            Record::Entity(EntityRef::Id(id)) => {
                out.push(tag::ENTITY);
                out.extend_from_slice(&id.to_bits().to_le_bytes());
            }
            Record::Entity(EntityRef::Outside) => out.push(tag::OUTSIDE),
            Record::Asset(uuid) => {
                out.push(tag::ASSET);
                out.extend_from_slice(uuid.as_bytes());
            }
        }
    }

    fn write_items(&mut self, out: &mut Vec<u8>, items: &[Record]) {
        write_varint(out, items.len() as u64);
        for item in items {
            self.write_value(out, item);
        }
    }
}

/// Zig-zag: maps signed to unsigned so small magnitudes stay short as varints
/// (0→0, -1→1, 1→2, -2→3).
fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// The inverse of [`zigzag`].
fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Writes a u64 as a varint to the output buffer.
/// Varints are a compact, variable-length encoding for unsigned integers.
/// The least significant 7 bits of each byte are used for the value, and the most significant bit
/// indicates whether there are more bytes to read.
fn write_varint(out: &mut Vec<u8>, mut v: u64) {
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

/// Writes a game save compactly: the scene it was taken against, the saved
/// world's order, the entities destroyed and created, the components
/// removed — then its changes and the scene's values before them, each as a
/// record is written, the first behind its length.
pub(super) fn encode_save(save: &SaveRecord) -> Vec<u8> {
    fn ids(out: &mut Vec<u8>, ids: &[PersistentId]) {
        write_varint(out, ids.len() as u64);
        for id in ids {
            out.extend_from_slice(&id.to_bits().to_le_bytes());
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(save.base.as_bytes());
    ids(&mut out, &save.order);
    ids(&mut out, &save.destroyed);
    ids(&mut out, &save.created);
    write_varint(&mut out, save.removed.len() as u64);
    for entry in &save.removed {
        out.extend_from_slice(&entry.entity.to_bits().to_le_bytes());
        write_varint(&mut out, entry.components.len() as u64);
        for name in &entry.components {
            write_varint(&mut out, name.len() as u64);
            out.extend_from_slice(name.as_bytes());
        }
    }
    let changes = encode(&save.changes);
    write_varint(&mut out, changes.len() as u64);
    out.extend(changes);
    out.extend(encode(&save.before));
    out
}

/// Reads a compactly written game save, as checked as a record is.
pub(super) fn decode_save(bytes: &[u8]) -> Result<SaveRecord, EncodingError> {
    fn ids(input: &mut Input<'_>) -> Result<Vec<PersistentId>, EncodingError> {
        let count = input.count(8)?;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(PersistentId::from_bits(input.u64_le()?));
        }
        Ok(ids)
    }
    let mut input = Input { bytes, at: 0 };
    let base = AssetUUID::from_bytes(input.array()?);
    let order = ids(&mut input)?;
    let destroyed = ids(&mut input)?;
    let created = ids(&mut input)?;
    // An entry is an id and a count at least.
    let removed_count = input.count(9)?;
    let mut removed = Vec::with_capacity(removed_count);
    for _ in 0..removed_count {
        let entity = PersistentId::from_bits(input.u64_le()?);
        let name_count = input.count(1)?;
        let mut components = Vec::with_capacity(name_count);
        for _ in 0..name_count {
            let len = input.count(1)?;
            let name = std::str::from_utf8(input.take(len)?)
                .map_err(|_| error("a component name that is not UTF-8".to_owned()))?;
            components.push(name.to_owned());
        }
        removed.push(RemovedComponents { entity, components });
    }
    Ok(SaveRecord {
        base,
        order,
        destroyed,
        created,
        removed,
        changes: {
            let len = input.count(1)?;
            decode(input.take(len)?)?
        },
        before: decode(&bytes[input.at..])?,
    })
}

/// Reads a compactly written record. A damaged input is an error, never a
/// panic, and never an allocation sized by a count the input does not back.
pub(super) fn decode(bytes: &[u8]) -> Result<SceneRecord, EncodingError> {
    let mut input = Input { bytes, at: 0 };
    let version = input.byte()?;
    if version != VERSION {
        return Err(error(format!(
            "compact layout version {version}, expected {VERSION}"
        )));
    }

    let symbol_count = input.count(1)?;
    let mut symbols = Vec::with_capacity(symbol_count);
    for _ in 0..symbol_count {
        let len = input.count(1)?;
        let text = std::str::from_utf8(input.take(len)?)
            .map_err(|_| error("a name that is not UTF-8".to_owned()))?;
        symbols.push(text.to_owned());
    }

    let shape_count = input.count(2)?;
    let mut shapes = Vec::with_capacity(shape_count);
    for _ in 0..shape_count {
        let shape = match input.byte()? {
            0 => {
                let name = input.symbol(&symbols)?;
                let field_count = input.count(1)?;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    fields.push(input.symbol(&symbols)?);
                }
                DecodedShape::Struct { name, fields }
            }
            1 => DecodedShape::Variant {
                enum_name: input.symbol(&symbols)?,
                variant: input.symbol(&symbols)?,
            },
            other => return Err(error(format!("unknown shape kind {other}"))),
        };
        shapes.push(shape);
    }

    let reader = Reader {
        symbols: &symbols,
        shapes: &shapes,
        names_left: Cell::new(bytes.len().saturating_mul(NAME_BYTES_PER_FILE_BYTE)),
    };
    let entity_count = input.count(8)?;
    let mut entities = Vec::with_capacity(entity_count);
    for _ in 0..entity_count {
        entities.push(PersistentId::from_bits(input.u64_le()?));
    }

    let page_count = input.count(2)?;
    let mut pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        let component_count = input.count(1)?;
        let mut components = Vec::with_capacity(component_count);
        for _ in 0..component_count {
            components.push(reader.name(input.symbol(&symbols)?)?);
        }
        let row_count = input.count(8)?;
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            rows.push(PersistentId::from_bits(input.u64_le()?));
        }
        // Every value takes at least a byte: a page whose rows times columns
        // exceed what is left cannot be real.
        if component_count
            .checked_mul(row_count)
            .is_none_or(|values| values > input.left())
        {
            return Err(error(format!(
                "a page of {row_count} rows by {component_count} columns the file has no room for"
            )));
        }
        let mut columns = Vec::with_capacity(component_count);
        for _ in 0..component_count {
            let mut column = Vec::with_capacity(row_count);
            for _ in 0..row_count {
                column.push(reader.value(&mut input)?);
            }
            columns.push(column);
        }
        pages.push(PageRecord {
            components,
            rows,
            columns,
        });
    }
    if input.at != bytes.len() {
        return Err(error(format!(
            "{} byte(s) after the last page",
            bytes.len() - input.at
        )));
    }
    Ok(SceneRecord { entities, pages })
}

enum DecodedShape {
    Struct { name: usize, fields: Vec<usize> },
    Variant { enum_name: usize, variant: usize },
}

fn error(message: String) -> EncodingError {
    EncodingError(message)
}

struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Input<'a> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn byte(&mut self) -> Result<u8, EncodingError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| error("the file ends early".to_owned()))?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], EncodingError> {
        if len > self.left() {
            return Err(error("the file ends early".to_owned()));
        }
        let taken = &self.bytes[self.at..self.at + len];
        self.at += len;
        Ok(taken)
    }

    fn varint(&mut self) -> Result<u64, EncodingError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err(error("a number too large for 64 bits".to_owned()));
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(error("a number too large for 64 bits".to_owned()))
    }

    /// A count of things each taking at least `each` bytes: refused when the
    /// bytes left could not hold that many.
    fn count(&mut self, each: usize) -> Result<usize, EncodingError> {
        let count = self.varint()?;
        let count = usize::try_from(count).map_err(|_| error("a count too large".to_owned()))?;
        if count
            .checked_mul(each)
            .is_none_or(|needed| needed > self.left())
        {
            return Err(error(format!(
                "a count of {count} the file has no room for"
            )));
        }
        Ok(count)
    }

    fn symbol(&mut self, symbols: &[String]) -> Result<usize, EncodingError> {
        let id = usize::try_from(self.varint()?).unwrap_or(usize::MAX);
        if id >= symbols.len() {
            return Err(error(format!("name {id} is not in the name table")));
        }
        Ok(id)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], EncodingError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn u64_le(&mut self) -> Result<u64, EncodingError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
}
