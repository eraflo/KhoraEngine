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

//! The trust boundary: how much the engine trusts a file it reads, and what
//! it refuses.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod guard;
mod limit_exceeded;
mod limits;
mod project;
mod quarantine;
mod store;

pub use guard::{FileGuard, ReadError};
pub use limits::{LimitExceeded, Limits};
pub use project::ProjectTrustState;
pub use quarantine::{QuarantineKey, Refusals};
pub use store::{ProjectTrust, TrustStore};

/// How much a file is trusted, from the most to the least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trust {
    /// The engine's own files.
    Engine,
    /// A file of a project its user trusts.
    Project,
    /// A file of a project its user has not trusted.
    Restricted,
    /// A file brought in from outside the project, or carrying the operating
    /// system's download mark.
    Imported,
    /// A game save.
    Save,
}

/// What a file is, by its extension, for its size cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// An image.
    Texture,
    /// A glTF file, or a buffer it reads.
    Gltf,
    /// A sound.
    Audio,
    /// A font.
    Font,
    /// A text format: script, shader, scene, material.
    Text,
    /// Anything else.
    Other,
}

impl FileKind {
    /// Returns the kind of the file at `path`, by its extension.
    pub fn of_path(path: &Path) -> FileKind {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return FileKind::Other;
        };
        match extension.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" | "gif" | "webp" | "tif" | "tiff"
            | "dds" | "ktx2" | "exr" => FileKind::Texture,
            "gltf" | "glb" | "bin" | "obj" => FileKind::Gltf,
            "wav" | "ogg" | "mp3" | "flac" => FileKind::Audio,
            "ttf" | "otf" => FileKind::Font,
            "erg" | "wgsl" | "kscene" | "kmat" | "kprefab" | "ron" | "json" | "toml" | "txt" => {
                FileKind::Text
            }
            _ => FileKind::Other,
        }
    }
}

impl Trust {
    /// Whether a file read at this trust has the strict caps: anything but
    /// the engine's own files and a trusted project's.
    pub fn is_strict(self) -> bool {
        !matches!(self, Trust::Engine | Trust::Project)
    }
}

/// One mebibyte.
const MIB: u64 = 1 << 20;

/// Returns the size, in bytes, past which a file of `kind` read at `trust` is refused.
///
/// See the book: concepts/trust.md, "The guard".
pub fn size_cap(kind: FileKind, trust: Trust) -> u64 {
    let (larger, strict) = match kind {
        FileKind::Texture => (256, 64),
        FileKind::Gltf => (1024, 256),
        FileKind::Audio => (512, 128),
        FileKind::Font | FileKind::Text | FileKind::Other => (64, 16),
    };
    if trust.is_strict() {
        strict * MIB
    } else {
        larger * MIB
    }
}

/// Whether `head`, the start of a file, reads as text: UTF-8 with no NUL.
///
/// A head cut inside its last character is still text: it starts a longer file.
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // `error_len` is `None` when the bytes end inside a character.
        Err(error) => error.error_len().is_none(),
    }
}

/// Where a refusal was decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrustLayer {
    /// Reading the file.
    File,
    /// Decoding it.
    Decoder,
    /// Reading it out of a pack.
    Pack,
    /// What it holds.
    Content,
}

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityReason {
    /// Its path leaves its root.
    Escape,
    /// It is larger than its cap.
    TooLarge {
        /// Its size, in bytes.
        size: u64,
        /// Its cap, in bytes.
        cap: u64,
    },
    /// Its bytes are not what its type says.
    WrongType {
        /// The type it was read as.
        expected: String,
    },
    /// Its content was refused before, and has not changed since.
    Quarantined,
    /// Decoding it went past one of its limits.
    OverLimit {
        /// What went past its limit.
        what: String,
        /// The limit it went past.
        limit: u64,
    },
    /// Its decoder panicked on it.
    DecoderPanic {
        /// What the panic said.
        message: String,
    },
}

/// A file the engine refused to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecurityRefusal {
    /// The file, as it was given.
    pub path: PathBuf,
    /// How much it was trusted.
    pub trust: Trust,
    /// Where it was refused.
    pub layer: TrustLayer,
    /// Why.
    pub reason: SecurityReason,
}

impl std::fmt::Display for SecurityRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.path.display(), self.reason)
    }
}

impl std::fmt::Display for SecurityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityReason::Escape => write!(f, "leaves the folder it is read from"),
            SecurityReason::TooLarge { size, cap } => {
                write!(f, "is {size} bytes, over the {cap} its kind is allowed")
            }
            SecurityReason::WrongType { expected } => {
                write!(f, "does not hold what a {expected} file starts with")
            }
            SecurityReason::Quarantined => {
                write!(f, "was refused before and has not changed since")
            }
            SecurityReason::OverLimit { what, limit } => {
                write!(f, "decodes past a limit: {what} passes {limit}")
            }
            SecurityReason::DecoderPanic { message } => {
                write!(f, "made its decoder fail: {message}")
            }
        }
    }
}

impl SecurityReason {
    /// What to do about a file refused for this reason.
    pub fn remedy(&self) -> &'static str {
        match self {
            SecurityReason::Escape => {
                "Keep the file inside the project's folder: a path leaving it, or a link                  pointing out of it, is never read."
            }
            SecurityReason::TooLarge { .. } => {
                "Make the file smaller, or split it. A file from an untrusted project or                  downloaded from the internet has smaller caps."
            }
            SecurityReason::WrongType { .. } => {
                "Check the file: its extension does not match what it holds. Fix or                  replace it, and it is read again."
            }
            SecurityReason::Quarantined => {
                "Change the file, or release it from quarantine, to have it read again."
            }
            SecurityReason::OverLimit { .. } => {
                "Make the file smaller: fewer pixels, vertices or samples. A file from an \
                 untrusted project or downloaded from the internet has smaller limits."
            }
            SecurityReason::DecoderPanic { .. } => {
                "The file is damaged or made to break its decoder. Replace it, and it is \
                 read again."
            }
        }
    }
}

/// The first bytes of the formats several decoders share.
pub(crate) mod magic {
    /// A RIFF file holding a WAVE.
    pub(crate) fn is_wav(head: &[u8]) -> bool {
        head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WAVE"
    }

    /// An MP3: an ID3 tag, or an MPEG audio frame's sync word.
    pub(crate) fn is_mp3(head: &[u8]) -> bool {
        head.starts_with(b"ID3") || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
    }
}

#[cfg(test)]
mod tests;
