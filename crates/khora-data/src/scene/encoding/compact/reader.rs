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

//! Reading one compact value, without recursion.
//!
//! A value nests: a list of structs of variants. Reading it by recursion
//! costs a stack frame per level, and a frame is large in an unoptimised
//! build — deep enough a value, and a load overflows the thread that runs
//! it. So the reader keeps the values it has opened and not yet finished on
//! a stack of its own, on the heap: depth costs a few words per level, and
//! the only limit is `MAX_DEPTH`, the one every encoding holds to.

use std::cell::Cell;

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;

use super::{error, tag, DecodedShape, Input};
use crate::scene::encoding::EncodingError;
use crate::scene::record::{EntityRef, Record, VariantPayload, MAX_DEPTH};

/// How many bytes of names a file may decode into, per byte of file.
///
/// A value refers to a name in a byte or two of the file, but the decoded
/// record holds a full copy of it. Capping the copied bytes per file byte
/// stops a small file from decoding into a huge record. Real scenes stay far
/// below it: a name is an identifier, and each reference costs at least a
/// byte.
pub(super) const NAME_BYTES_PER_FILE_BYTE: usize = 64;

/// Reads values against a file's symbol and shape tables.
pub(super) struct Reader<'t> {
    pub(super) symbols: &'t [String],
    pub(super) shapes: &'t [DecodedShape],
    /// The bytes of names the file may still copy out of the symbol table.
    pub(super) names_left: Cell<usize>,
}

/// A value opened and waiting for its parts.
enum Open<'t> {
    Some,
    Newtype {
        name: String,
    },
    Seq {
        items: Vec<Record>,
        left: usize,
    },
    Map {
        entries: Vec<(Record, Record)>,
        key: Option<Record>,
        left: usize,
    },
    TupleStruct {
        name: String,
        items: Vec<Record>,
        left: usize,
    },
    Struct {
        name: String,
        fields: Vec<(String, Record)>,
        names: &'t [usize],
    },
    VariantNewtype {
        enum_name: String,
        variant: String,
    },
    VariantTuple {
        enum_name: String,
        variant: String,
        items: Vec<Record>,
        left: usize,
    },
    VariantStruct {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Record)>,
        names: &'t [usize],
    },
}

/// What reading a tag gave: a whole value, or one opened.
enum Started<'t> {
    Whole(Record),
    Opened(Open<'t>),
}

impl<'t> Reader<'t> {
    /// The name `id` stands for, charged to the file's allowance of names.
    pub(super) fn name(&self, id: usize) -> Result<String, EncodingError> {
        let name = &self.symbols[id];
        let left = self
            .names_left
            .get()
            .checked_sub(name.len())
            .ok_or_else(|| {
                error(format!(
                    "the values copy more than {NAME_BYTES_PER_FILE_BYTE} bytes of names \
                     per byte of file"
                ))
            })?;
        self.names_left.set(left);
        Ok(name.clone())
    }

    fn shape(&self, input: &mut Input<'_>) -> Result<&'t DecodedShape, EncodingError> {
        let id = usize::try_from(input.varint()?).unwrap_or(usize::MAX);
        self.shapes
            .get(id)
            .ok_or_else(|| error(format!("shape {id} is not in the shape table")))
    }

    /// Reads one whole value.
    pub(super) fn value(&self, input: &mut Input<'_>) -> Result<Record, EncodingError> {
        let mut open: Vec<Open<'t>> = Vec::new();
        'read: loop {
            // A value is as deep as the values open around it.
            if open.len() > MAX_DEPTH {
                return Err(error(format!(
                    "a value nests deeper than {MAX_DEPTH} levels"
                )));
            }
            let mut step = match self.start(input)? {
                Started::Whole(record) => Step::Done(record),
                Started::Opened(opened) => opened.opened(),
            };
            // Hand each finished value to the one around it, until one still
            // waits for more.
            loop {
                match step {
                    Step::Waiting(waiting) => {
                        open.push(waiting);
                        continue 'read;
                    }
                    Step::Done(done) => match open.pop() {
                        None => return Ok(done),
                        Some(around) => step = around.accept(done, self)?,
                    },
                }
            }
        }
    }

    /// Reads a tag and what directly follows it.
    fn start(&self, input: &mut Input<'_>) -> Result<Started<'t>, EncodingError> {
        let whole = match input.byte()? {
            tag::UNIT => Record::Unit,
            tag::FALSE => Record::Bool(false),
            tag::TRUE => Record::Bool(true),
            tag::I64 => Record::I64(super::unzigzag(input.varint()?)),
            tag::U64 => Record::U64(input.varint()?),
            tag::F32 => Record::F32(f32::from_le_bytes(input.array()?)),
            tag::F64 => Record::F64(f64::from_le_bytes(input.array()?)),
            tag::DECIMAL => Record::Decimal(f64::from_le_bytes(input.array()?)),
            tag::CHAR => {
                let code = u32::try_from(input.varint()?).unwrap_or(u32::MAX);
                Record::Char(char::from_u32(code).ok_or_else(|| error("a bad char".to_owned()))?)
            }
            tag::STR => {
                let len = input.count(1)?;
                let text = std::str::from_utf8(input.take(len)?)
                    .map_err(|_| error("a string that is not UTF-8".to_owned()))?;
                Record::Str(text.to_owned())
            }
            tag::BYTES => {
                let len = input.count(1)?;
                Record::Bytes(input.take(len)?.to_vec())
            }
            tag::NONE => Record::None,
            tag::SOME => return Ok(Started::Opened(Open::Some)),
            tag::SEQ => {
                let left = input.count(1)?;
                return Ok(Started::Opened(Open::Seq {
                    items: Vec::with_capacity(left),
                    left,
                }));
            }
            tag::MAP => {
                let left = input.count(2)?;
                return Ok(Started::Opened(Open::Map {
                    entries: Vec::with_capacity(left),
                    key: None,
                    left,
                }));
            }
            tag::UNIT_STRUCT => Record::UnitStruct {
                name: self.name(input.symbol(self.symbols)?)?,
            },
            tag::STRUCT => match self.shape(input)? {
                DecodedShape::Struct { name, fields } => {
                    let name = self.name(*name)?;
                    return Ok(Started::Opened(Open::Struct {
                        name,
                        fields: fields_for(fields, input)?,
                        names: fields,
                    }));
                }
                DecodedShape::Variant { .. } => {
                    return Err(error("a struct with a variant's shape".to_owned()))
                }
            },
            tag::TUPLE_STRUCT => {
                let name = self.name(input.symbol(self.symbols)?)?;
                let left = input.count(1)?;
                return Ok(Started::Opened(Open::TupleStruct {
                    name,
                    items: Vec::with_capacity(left),
                    left,
                }));
            }
            tag::NEWTYPE => {
                let name = self.name(input.symbol(self.symbols)?)?;
                return Ok(Started::Opened(Open::Newtype { name }));
            }
            tag::VARIANT => return self.variant(input),
            tag::ENTITY => Record::Entity(EntityRef::Id(PersistentId::from_bits(input.u64_le()?))),
            tag::OUTSIDE => Record::Entity(EntityRef::Outside),
            tag::ASSET => Record::Asset(AssetUUID::from_bytes(input.array()?)),
            other => return Err(error(format!("unknown value tag {other}"))),
        };
        Ok(Started::Whole(whole))
    }

    /// Reads a variant's shape and payload kind.
    fn variant(&self, input: &mut Input<'_>) -> Result<Started<'t>, EncodingError> {
        let (enum_name, variant) = match self.shape(input)? {
            DecodedShape::Variant { enum_name, variant } => {
                (self.name(*enum_name)?, self.name(*variant)?)
            }
            DecodedShape::Struct { .. } => {
                return Err(error("a variant with a struct's shape".to_owned()))
            }
        };
        Ok(match input.byte()? {
            tag::PAYLOAD_UNIT => Started::Whole(Record::Variant {
                enum_name,
                variant,
                payload: VariantPayload::Unit,
            }),
            tag::PAYLOAD_NEWTYPE => Started::Opened(Open::VariantNewtype { enum_name, variant }),
            tag::PAYLOAD_TUPLE => {
                let left = input.count(1)?;
                Started::Opened(Open::VariantTuple {
                    enum_name,
                    variant,
                    items: Vec::with_capacity(left),
                    left,
                })
            }
            tag::PAYLOAD_STRUCT => match self.shape(input)? {
                DecodedShape::Struct { fields, .. } => Started::Opened(Open::VariantStruct {
                    enum_name,
                    variant,
                    fields: fields_for(fields, input)?,
                    names: fields,
                }),
                DecodedShape::Variant { .. } => {
                    return Err(error("variant fields with a variant's shape".to_owned()))
                }
            },
            other => return Err(error(format!("unknown payload kind {other}"))),
        })
    }
}

/// Room for a struct's fields — once the file is known to hold a byte for
/// each of them.
fn fields_for(names: &[usize], input: &Input<'_>) -> Result<Vec<(String, Record)>, EncodingError> {
    if names.len() > input.left() {
        return Err(error("the file ends early".to_owned()));
    }
    Ok(Vec::with_capacity(names.len()))
}

/// Where a value stands once given a part, or once opened.
enum Step<'t> {
    /// Whole: it goes to the value around it.
    Done(Record),
    /// It waits for more parts.
    Waiting(Open<'t>),
}

impl<'t> Open<'t> {
    /// A value just opened: whole already if it has no parts.
    fn opened(self) -> Step<'t> {
        let empty = match &self {
            Self::Seq { left, .. }
            | Self::TupleStruct { left, .. }
            | Self::VariantTuple { left, .. }
            | Self::Map { left, .. } => *left == 0,
            Self::Struct { names, .. } | Self::VariantStruct { names, .. } => names.is_empty(),
            Self::Some | Self::Newtype { .. } | Self::VariantNewtype { .. } => false,
        };
        if empty {
            Step::Done(self.close())
        } else {
            Step::Waiting(self)
        }
    }

    /// Takes the next part.
    fn accept(mut self, part: Record, reader: &Reader<'_>) -> Result<Step<'t>, EncodingError> {
        match &mut self {
            Self::Some => return Ok(Step::Done(Record::Some(Box::new(part)))),
            Self::Newtype { name } => {
                return Ok(Step::Done(Record::Newtype {
                    name: std::mem::take(name),
                    value: Box::new(part),
                }))
            }
            Self::VariantNewtype { enum_name, variant } => {
                return Ok(Step::Done(Record::Variant {
                    enum_name: std::mem::take(enum_name),
                    variant: std::mem::take(variant),
                    payload: VariantPayload::Newtype(Box::new(part)),
                }))
            }
            Self::Seq { items, left }
            | Self::TupleStruct { items, left, .. }
            | Self::VariantTuple { items, left, .. } => {
                items.push(part);
                *left -= 1;
            }
            Self::Map { entries, key, left } => match key.take() {
                None => *key = Some(part),
                Some(key) => {
                    entries.push((key, part));
                    *left -= 1;
                }
            },
            Self::Struct { fields, names, .. } | Self::VariantStruct { fields, names, .. } => {
                let name = reader.name(names[fields.len()])?;
                fields.push((name, part));
                if fields.len() == names.len() {
                    return Ok(Step::Done(self.close()));
                }
                return Ok(Step::Waiting(self));
            }
        }
        Ok(self.opened())
    }

    /// The value, whole. A one-part value is closed by `accept`, never here.
    fn close(self) -> Record {
        match self {
            Self::Seq { items, .. } => Record::Seq(items),
            Self::Map { entries, .. } => Record::Map(entries),
            Self::TupleStruct { name, items, .. } => Record::TupleStruct {
                name,
                fields: items,
            },
            Self::Struct { name, fields, .. } => Record::Struct { name, fields },
            Self::VariantTuple {
                enum_name,
                variant,
                items,
                ..
            } => Record::Variant {
                enum_name,
                variant,
                payload: VariantPayload::Tuple(items),
            },
            Self::VariantStruct {
                enum_name,
                variant,
                fields,
                ..
            } => Record::Variant {
                enum_name,
                variant,
                payload: VariantPayload::Struct(fields),
            },
            // Opened with nothing, these have no part to close over; `opened`
            // never calls this for them.
            Self::Some | Self::Newtype { .. } | Self::VariantNewtype { .. } => Record::Unit,
        }
    }
}
