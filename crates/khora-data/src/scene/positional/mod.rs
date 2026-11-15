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

//! Values by position, no names: a snapshot's bytes, read only by the schema that wrote them.
//!
//! An entity reference is 0 (outside) or 1 then its persistent id, 8 bytes
//! little-endian; a `u8` is one raw byte. See the book: reference/formats.md, "Snapshot layout".

mod de;
mod ser;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::record::{RecordError, ReferenceReader, ReferenceWriter};

/// Appends `value` to `out`, positionally, its entities through `references`.
///
/// # Errors
///
/// A value the reader would refuse: nested too deep, a sequence of unknown length
/// or of too many empty elements, a malformed entity. `out` is then left as it was.
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

/// Reads a `T` from `bytes`, which it must consume exactly, its entities through `references`.
///
/// # Errors
///
/// Damaged or trailing bytes, or a reference `references` refuses; never a panic.
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

/// How many elements a sequence may hold beyond the bytes they take.
///
/// Room for elements that take no bytes (a unit, a marker), and no more. The
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
