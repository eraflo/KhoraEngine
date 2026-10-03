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

//! The forms a scene record is written in.
//!
//! One record, several encodings: which one is a matter of how the file will
//! be used — read by people, kept small, read by other tools — never of what
//! it holds.

mod compact;

use serde::Deserialize;

use super::save::SaveRecord;
use super::scene_record::SceneRecord;

/// One way of writing a scene record as bytes.
pub trait SceneEncoding {
    /// The id the file header names this encoding by.
    fn id(&self) -> &'static str;
    /// Writes `record`.
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError>;
    /// Reads a record back. A damaged input is an error, never a panic.
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError>;
    /// Writes a game save.
    fn encode_save(&self, save: &SaveRecord) -> Result<Vec<u8>, EncodingError>;
    /// Reads a game save back. A damaged input is an error, never a panic.
    fn decode_save(&self, bytes: &[u8]) -> Result<SaveRecord, EncodingError>;
}

/// Why bytes could not be written or read.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodingError(pub String);

impl std::fmt::Display for EncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EncodingError {}

/// Compact binary: every name once, values by position.
pub struct CompactEncoding;

/// Pretty JSON, for people.
pub struct TextEncoding;

/// MessagePack with names, for other tools.
pub struct MsgPackEncoding;

impl SceneEncoding for CompactEncoding {
    fn id(&self) -> &'static str {
        "KH_COMPACT_V2"
    }
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError> {
        Ok(compact::encode(record))
    }
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError> {
        compact::decode(bytes)
    }
    fn encode_save(&self, save: &SaveRecord) -> Result<Vec<u8>, EncodingError> {
        Ok(compact::encode_save(save))
    }
    fn decode_save(&self, bytes: &[u8]) -> Result<SaveRecord, EncodingError> {
        compact::decode_save(bytes)
    }
}

impl SceneEncoding for TextEncoding {
    fn id(&self) -> &'static str {
        "KH_TEXT_V2"
    }
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError> {
        serde_json::to_vec_pretty(record).map_err(|error| EncodingError(error.to_string()))
    }
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError> {
        // serde_json's own recursion limit also counts the scene document's
        // fixed levels (pages, columns, rows), so it would cut a value off
        // below `MAX_DEPTH`. It is disabled: every value is read through the
        // record reader, which enforces `MAX_DEPTH` itself, as every
        // encoding does.
        let mut reader = serde_json::Deserializer::from_slice(bytes);
        reader.disable_recursion_limit();
        let record = SceneRecord::deserialize(&mut reader)
            .map_err(|error| EncodingError(error.to_string()))?;
        reader
            .end()
            .map_err(|error| EncodingError(error.to_string()))?;
        Ok(record)
    }
    fn encode_save(&self, save: &SaveRecord) -> Result<Vec<u8>, EncodingError> {
        serde_json::to_vec_pretty(save).map_err(|error| EncodingError(error.to_string()))
    }
    fn decode_save(&self, bytes: &[u8]) -> Result<SaveRecord, EncodingError> {
        // As a scene: every value is read through the record reader, which
        // holds the depth limit itself.
        let mut reader = serde_json::Deserializer::from_slice(bytes);
        reader.disable_recursion_limit();
        let save = SaveRecord::deserialize(&mut reader)
            .map_err(|error| EncodingError(error.to_string()))?;
        reader
            .end()
            .map_err(|error| EncodingError(error.to_string()))?;
        Ok(save)
    }
}

impl SceneEncoding for MsgPackEncoding {
    fn id(&self) -> &'static str {
        "KH_MSGPACK_V2"
    }
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError> {
        rmp_serde::to_vec_named(record).map_err(|error| EncodingError(error.to_string()))
    }
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError> {
        rmp_serde::from_slice(bytes).map_err(|error| EncodingError(error.to_string()))
    }
    fn encode_save(&self, save: &SaveRecord) -> Result<Vec<u8>, EncodingError> {
        rmp_serde::to_vec_named(save).map_err(|error| EncodingError(error.to_string()))
    }
    fn decode_save(&self, bytes: &[u8]) -> Result<SaveRecord, EncodingError> {
        rmp_serde::from_slice(bytes).map_err(|error| EncodingError(error.to_string()))
    }
}

/// The encoding a file header names.
pub fn encoding_named(id: &str) -> Option<&'static dyn SceneEncoding> {
    [
        &CompactEncoding as &'static dyn SceneEncoding,
        &TextEncoding,
        &MsgPackEncoding,
    ]
    .into_iter()
    .find(|encoding| encoding.id() == id)
}

#[cfg(test)]
mod tests;
