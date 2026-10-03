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

//! Values by position: the bytes a snapshot holds.
//!
//! Not self-describing — fields in order, no names — so a value is read only
//! by the schema that wrote it. Entity and asset references are intercepted
//! by their reserved serde names, exactly as the record codec does.
//!
//! The format: counts and integers are LEB128 varints (signed ones
//! zig-zagged, a `u8` a raw byte), floats little-endian at their width, a
//! `bool` and an option's presence one byte that is 0 or 1, a string or a
//! byte string its length then its bytes, a sequence or a map its count
//! then its elements, a struct or a tuple its fields in order, an enum its
//! variant index then its payload. An entity reference is 0 (outside) or 1
//! followed by its persistent id, 8 bytes little-endian.

mod de;
mod ser;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::record::{RecordError, ReferenceReader, ReferenceWriter};

/// Appends `value` to `out`, positionally, its entities through `references`.
pub fn to_positional<T: Serialize + ?Sized>(
    value: &T,
    out: &mut Vec<u8>,
    references: &mut dyn ReferenceWriter,
) -> Result<(), RecordError> {
    let start = out.len();
    let written = value.serialize(&mut ser::Writer::new(out, references));
    if written.is_err() {
        // Nothing half-written stays behind.
        out.truncate(start);
    }
    written
}

/// Reads a `T` from `bytes`, which it must consume exactly, its entities
/// through `references`. A damaged input is an error, never a panic.
pub fn from_positional<T: DeserializeOwned>(
    bytes: &[u8],
    references: &mut dyn ReferenceReader,
) -> Result<T, RecordError> {
    let mut reader = de::Reader::new(bytes, references);
    let value = T::deserialize(&mut reader)?;
    reader.finish()?;
    Ok(value)
}

/// Appends `value` as a LEB128 varint.
pub(crate) fn write_varint(out: &mut Vec<u8>, mut value: u64) {
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

/// How many elements a sequence may hold beyond the bytes they take: room
/// for elements that take no bytes (a unit, a marker), and no more. The
/// writer refuses a longer one, so whatever is written reads back.
pub(crate) const FREE_ELEMENTS: usize = 256;

/// Zig-zag: maps signed to unsigned so small magnitudes stay short as
/// varints (0→0, -1→1, 1→2, -2→3).
fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// The inverse of [`zigzag`].
fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[cfg(test)]
mod tests;
