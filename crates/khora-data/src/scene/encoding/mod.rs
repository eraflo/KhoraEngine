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

//! The byte forms a scene record is written in; each carries the same record.
//!
//! See the book: concepts/serialization.md, "Goals and encodings".

mod compact;

use serde::Deserialize;

use super::save::SaveRecord;
use super::scene_record::SceneRecord;

/// One way of writing a scene record as bytes.
pub trait SceneEncoding {
    /// Returns the id the file header names this encoding by.
    fn id(&self) -> &'static str;
    /// Writes `record`.
    ///
    /// # Errors
    ///
    /// When the record cannot be written in this encoding.
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError>;
    /// Reads a record back.
    ///
    /// # Errors
    ///
    /// When `bytes` are damaged or not a record in this encoding: an error, never a panic.
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError>;
    /// Writes a game save.
    ///
    /// # Errors
    ///
    /// When the save cannot be written in this encoding.
    fn encode_save(&self, save: &SaveRecord) -> Result<Vec<u8>, EncodingError>;
    /// Reads a game save back.
    ///
    /// # Errors
    ///
    /// When `bytes` are damaged or not a save in this encoding: an error, never a panic.
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

/// Pretty JSON, for people.
pub struct TextEncoding;

impl SceneEncoding for TextEncoding {
    fn id(&self) -> &'static str {
        "KH_TEXT_V2"
    }
    fn encode(&self, record: &SceneRecord) -> Result<Vec<u8>, EncodingError> {
        serde_json::to_vec_pretty(record).map_err(|error| EncodingError(error.to_string()))
    }
    fn decode(&self, bytes: &[u8]) -> Result<SceneRecord, EncodingError> {
        // serde_json's recursion limit also counts the document's fixed levels
        // (pages, columns, rows), so it would cut a value off below `MAX_DEPTH`.
        // The record reader every value goes through enforces `MAX_DEPTH`
        // itself, as in every encoding.
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

/// MessagePack with names, for other tools.
pub struct MsgPackEncoding;

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

/// Returns the encoding a file header names, if it is one of the record encodings.
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
