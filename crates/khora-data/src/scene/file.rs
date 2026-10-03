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

//! A scene record in a scene file: header, then one encoding of the record.

use khora_core::scene::{
    SceneFile, SceneHeader, HEADER_MAGIC_BYTES, SAVE_MAGIC_BYTES, SCENE_FORMAT_VERSION,
};

use super::encoding::{encoding_named, EncodingError, SceneEncoding};
use super::scene_record::SceneRecord;

/// Why a scene file could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneFileReadError {
    /// Written in a format this engine no longer reads.
    OldFormat(u8),
    /// Written in a format newer than this engine.
    NewerFormat(u8),
    /// The header names an encoding this engine does not have.
    UnknownEncoding(String),
    /// The payload could not be decoded.
    Encoding(EncodingError),
    /// A scene was expected, and the file is a game save.
    NotAScene,
    /// A game save was expected, and the file is a scene.
    NotASave,
}

impl std::fmt::Display for SceneFileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OldFormat(version) => write!(
                f,
                "scene format v{version} is older than any this engine reads"
            ),
            Self::NewerFormat(version) => write!(
                f,
                "scene format v{version} is newer than this engine (v{SCENE_FORMAT_VERSION})"
            ),
            Self::UnknownEncoding(id) => write!(f, "no scene encoding `{id}`"),
            Self::Encoding(error) => write!(f, "{error}"),
            Self::NotAScene => f.write_str("a game save, where a scene was expected"),
            Self::NotASave => f.write_str("a scene, where a game save was expected"),
        }
    }
}

impl std::error::Error for SceneFileReadError {}

/// `record`, written with `encoding` behind a scene header.
pub fn write_scene_file(
    record: &SceneRecord,
    encoding: &dyn SceneEncoding,
) -> Result<SceneFile, EncodingError> {
    scene_file(encoding.id(), encoding.encode(record)?)
}

/// `payload`, behind a scene header naming `encoding_id`.
pub(super) fn scene_file(encoding_id: &str, payload: Vec<u8>) -> Result<SceneFile, EncodingError> {
    file_with(HEADER_MAGIC_BYTES, encoding_id, payload)
}

/// `payload`, behind a header of `magic` naming `encoding_id`.
pub(super) fn file_with(
    magic: [u8; 8],
    encoding_id: &str,
    payload: Vec<u8>,
) -> Result<SceneFile, EncodingError> {
    let id = encoding_id.as_bytes();
    let mut header_id = [0u8; 32];
    if id.len() > header_id.len() {
        return Err(EncodingError(format!(
            "the encoding id `{encoding_id}` is longer than the {} bytes a header holds",
            header_id.len()
        )));
    }
    header_id[..id.len()].copy_from_slice(id);
    Ok(SceneFile {
        header: SceneHeader {
            magic_bytes: magic,
            format_version: SCENE_FORMAT_VERSION,
            encoding_id: header_id,
            payload_length: payload.len() as u64,
        },
        payload,
    })
}

/// The encoding a scene file's header names.
pub fn encoding_of(file: &SceneFile) -> String {
    String::from_utf8_lossy(&file.header.encoding_id)
        .trim_end_matches('\0')
        .to_owned()
}

/// Refuses a file that is not a scene, or is written in another scene format
/// than this engine's.
pub(super) fn check_format(file: &SceneFile) -> Result<(), SceneFileReadError> {
    if file.header.magic_bytes != HEADER_MAGIC_BYTES {
        return Err(SceneFileReadError::NotAScene);
    }
    check_version(file)
}

/// Refuses a file that is not a game save, or is written in another format
/// than this engine's.
pub(super) fn check_save_format(file: &SceneFile) -> Result<(), SceneFileReadError> {
    if file.header.magic_bytes != SAVE_MAGIC_BYTES {
        return Err(SceneFileReadError::NotASave);
    }
    check_version(file)
}

fn check_version(file: &SceneFile) -> Result<(), SceneFileReadError> {
    let version = file.header.format_version;
    if version < SCENE_FORMAT_VERSION {
        return Err(SceneFileReadError::OldFormat(version));
    }
    if version > SCENE_FORMAT_VERSION {
        return Err(SceneFileReadError::NewerFormat(version));
    }
    Ok(())
}

/// The record a scene file holds.
pub fn read_scene_file(file: &SceneFile) -> Result<SceneRecord, SceneFileReadError> {
    check_format(file)?;
    let id = encoding_of(file);
    let encoding = encoding_named(&id).ok_or(SceneFileReadError::UnknownEncoding(id))?;
    encoding
        .decode(&file.payload)
        .map_err(SceneFileReadError::Encoding)
}
