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

use khora_core::scene::{SceneFile, SceneHeader, HEADER_MAGIC_BYTES, SCENE_FORMAT_VERSION};

use super::encoding::{encoding_named, EncodingError, SceneEncoding};
use super::scene_record::SceneRecord;

/// The command that converts a project's scenes written before scene records.
pub const UPGRADE_SCENES_COMMAND: &str = "cargo xtask assets upgrade-scenes <project>";

/// Why a scene file could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneFileReadError {
    /// Written in a format this engine no longer reads.
    OldFormat {
        /// The version the file was written in.
        version: u8,
        /// What converts it — the error carries its remedy wherever it is shown.
        upgrade: &'static str,
    },
    /// Written in a format newer than this engine.
    NewerFormat(u8),
    /// The header names an encoding this engine does not have.
    UnknownEncoding(String),
    /// The payload could not be decoded.
    Encoding(EncodingError),
}

impl std::fmt::Display for SceneFileReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OldFormat { version, upgrade } => write!(
                f,
                "scene format v{version} predates scene records; convert the project once with \
                 `{upgrade}`"
            ),
            Self::NewerFormat(version) => write!(
                f,
                "scene format v{version} is newer than this engine (v{SCENE_FORMAT_VERSION})"
            ),
            Self::UnknownEncoding(id) => write!(f, "no scene encoding `{id}`"),
            Self::Encoding(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SceneFileReadError {}

/// `record`, written with `encoding` behind a scene header.
pub fn write_scene_file(
    record: &SceneRecord,
    encoding: &dyn SceneEncoding,
) -> Result<SceneFile, EncodingError> {
    let payload = encoding.encode(record)?;
    let id = encoding.id().as_bytes();
    let mut encoding_id = [0u8; 32];
    if id.len() > encoding_id.len() {
        return Err(EncodingError(format!(
            "the encoding id `{}` is longer than the {} bytes a header holds",
            encoding.id(),
            encoding_id.len()
        )));
    }
    encoding_id[..id.len()].copy_from_slice(id);
    Ok(SceneFile {
        header: SceneHeader {
            magic_bytes: HEADER_MAGIC_BYTES,
            format_version: SCENE_FORMAT_VERSION,
            encoding_id,
            payload_length: payload.len() as u64,
        },
        payload,
    })
}

/// The record a scene file holds.
pub fn read_scene_file(file: &SceneFile) -> Result<SceneRecord, SceneFileReadError> {
    let version = file.header.format_version;
    if version < SCENE_FORMAT_VERSION {
        return Err(SceneFileReadError::OldFormat {
            version,
            upgrade: UPGRADE_SCENES_COMMAND,
        });
    }
    if version > SCENE_FORMAT_VERSION {
        return Err(SceneFileReadError::NewerFormat(version));
    }
    let id = String::from_utf8_lossy(&file.header.encoding_id)
        .trim_end_matches('\0')
        .to_owned();
    let encoding = encoding_named(&id).ok_or(SceneFileReadError::UnknownEncoding(id))?;
    encoding
        .decode(&file.payload)
        .map_err(SceneFileReadError::Encoding)
}
