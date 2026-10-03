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

//! Defines the unified file format for Khora scenes.
//!
//! Every saved scene uses this container: a fixed-size [`SceneHeader`] followed
//! by a variable-length payload. The header names the encoding the payload was
//! written with, so a file is read back without knowing in advance how it was
//! saved.

use std::convert::TryInto;

/// A unique byte sequence to identify Khora Scene Files. ("KHORASCN").
pub const HEADER_MAGIC_BYTES: [u8; 8] = *b"KHORASCN";

/// The magic bytes of a game save ("KHORASAV"): the same header as a scene,
/// told apart so a save is never read where a scene is expected, nor the
/// reverse.
pub const SAVE_MAGIC_BYTES: [u8; 8] = *b"KHORASAV";

/// The scene file format this engine writes and reads.
///
/// Version 2 holds scene records: components by name, entities by persistent
/// identity. How a component's fields evolve is carried by the records
/// themselves, so this number moves only when the file's own layout does.
pub const SCENE_FORMAT_VERSION: u8 = 2;
const ENCODING_ID_LEN: usize = 32;

/// An error that can occur when parsing a `SceneFile` from bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneFileError {
    /// The bytes end before the header does, or before the payload the
    /// header announces.
    TooShort,
    /// The file's magic bytes are neither `HEADER_MAGIC_BYTES` nor
    /// `SAVE_MAGIC_BYTES`: not a Khora scene or save file.
    InvalidMagicBytes,
}

/// The fixed-size header at the beginning of every Khora scene file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneHeader {
    /// Magic bytes to identify the file type: `HEADER_MAGIC_BYTES` for a
    /// scene, `SAVE_MAGIC_BYTES` for a game save.
    pub magic_bytes: [u8; 8],
    /// The scene format the file was written in (see `SCENE_FORMAT_VERSION`).
    pub format_version: u8,
    /// A null-padded UTF-8 string naming the encoding of the payload,
    /// e.g. "KH_COMPACT_V2", "KH_TEXT_V2".
    pub encoding_id: [u8; ENCODING_ID_LEN],
    /// The length of the payload data that follows this header, in bytes.
    pub payload_length: u64,
}

/// A logical representation of a full scene file in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFile {
    /// The parsed header data.
    pub header: SceneHeader,
    /// The raw, variable-length payload data.
    pub payload: Vec<u8>,
}

// NOTE: We are intentionally not using `serde` for the header.
// It's a fixed-layout, performance-critical part of the file format,
// so direct byte manipulation is more robust and efficient.
impl SceneHeader {
    /// The total size of the header in bytes.
    pub const SIZE: usize = 8 + 1 + ENCODING_ID_LEN + 8;

    /// Attempts to parse a `SceneHeader` from the beginning of a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < Self::SIZE {
            return Err("Not enough bytes to form a valid header");
        }

        let magic_bytes: [u8; 8] = bytes[0..8].try_into().unwrap();
        if magic_bytes != HEADER_MAGIC_BYTES && magic_bytes != SAVE_MAGIC_BYTES {
            return Err("Invalid magic bytes; not a Khora scene or save file");
        }

        let format_version = bytes[8];

        let encoding_id: [u8; ENCODING_ID_LEN] = bytes[9..9 + ENCODING_ID_LEN].try_into().unwrap();

        let payload_length =
            u64::from_le_bytes(bytes[9 + ENCODING_ID_LEN..Self::SIZE].try_into().unwrap());

        Ok(Self {
            magic_bytes,
            format_version,
            encoding_id,
            payload_length,
        })
    }

    /// Serializes the header into a fixed-size byte array.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.magic_bytes);
        bytes[8] = self.format_version;
        bytes[9..9 + ENCODING_ID_LEN].copy_from_slice(&self.encoding_id);
        let payload_bytes = self.payload_length.to_le_bytes();
        bytes[9 + ENCODING_ID_LEN..Self::SIZE].copy_from_slice(&payload_bytes);
        bytes
    }
}

impl SceneFile {
    /// Parses a `SceneFile` from a byte slice.
    ///
    /// The bytes are input: a damaged length is an error, never an overflow.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SceneFileError> {
        if bytes.len() < SceneHeader::SIZE {
            return Err(SceneFileError::TooShort);
        }
        let header =
            SceneHeader::from_bytes(bytes).map_err(|_| SceneFileError::InvalidMagicBytes)?;
        let header_size = SceneHeader::SIZE;
        let payload_end = usize::try_from(header.payload_length)
            .ok()
            .and_then(|length| header_size.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or(SceneFileError::TooShort)?;

        let payload = bytes[header_size..payload_end].to_vec();
        Ok(Self { header, payload })
    }

    /// Serializes the entire `SceneFile` (header + payload) into a single byte vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_bytes = self.header.to_bytes();
        let mut file_bytes = Vec::with_capacity(header_bytes.len() + self.payload.len());
        file_bytes.extend_from_slice(&header_bytes);
        file_bytes.extend_from_slice(&self.payload);
        file_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of `payload`, with its header's length field set to `length`.
    fn bytes_claiming(length: u64, payload: &[u8]) -> Vec<u8> {
        let header = SceneHeader {
            magic_bytes: HEADER_MAGIC_BYTES,
            format_version: SCENE_FORMAT_VERSION,
            encoding_id: [0; ENCODING_ID_LEN],
            payload_length: length,
        };
        let mut bytes = header.to_bytes().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    /// A file round-trips through its bytes.
    #[test]
    fn a_scene_file_round_trips_through_its_bytes() {
        let bytes = bytes_claiming(3, &[1, 2, 3]);
        let file = SceneFile::from_bytes(&bytes).expect("a whole file parses");
        assert_eq!(file.payload, vec![1, 2, 3]);
        assert_eq!(file.to_bytes(), bytes);
    }

    /// A length past the end — however large — is an error, never an
    /// overflow or a panic: the bytes are input.
    #[test]
    fn a_damaged_payload_length_is_too_short_not_a_panic() {
        for length in [4, u64::MAX, u64::MAX - SceneHeader::SIZE as u64 + 1] {
            assert_eq!(
                SceneFile::from_bytes(&bytes_claiming(length, &[1, 2, 3])),
                Err(SceneFileError::TooShort),
                "length {length}"
            );
        }
    }

    /// A save shares the scene header, under its own magic: its bytes parse
    /// back to the same file, the magic kept, so a reader can tell the two
    /// apart after parsing.
    #[test]
    fn a_save_file_round_trips_through_its_bytes() {
        let mut bytes = bytes_claiming(2, &[7, 9]);
        bytes[..8].copy_from_slice(&SAVE_MAGIC_BYTES);

        let file = SceneFile::from_bytes(&bytes).expect("a save header parses");
        assert_eq!(file.header.magic_bytes, SAVE_MAGIC_BYTES);
        assert_eq!(file.payload, vec![7, 9]);
        assert_eq!(file.to_bytes(), bytes);
    }

    /// Too few bytes for a header is `TooShort`; a header whose magic is
    /// wrong is not a scene file.
    #[test]
    fn a_short_or_foreign_file_names_its_fault() {
        assert_eq!(
            SceneFile::from_bytes(&[0; 4]),
            Err(SceneFileError::TooShort)
        );
        let mut foreign = bytes_claiming(0, &[]);
        foreign[0] = b'X';
        assert_eq!(
            SceneFile::from_bytes(&foreign),
            Err(SceneFileError::InvalidMagicBytes)
        );
    }
}
