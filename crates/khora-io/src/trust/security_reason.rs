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

//! Why a file was refused, and what to do about it.

use serde::{Deserialize, Serialize};

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
    /// A pack entry whose bytes are not those its signed manifest records.
    Tampered,
    /// Its content does not read as what it is.
    Malformed {
        /// What it was read as.
        expected: String,
        /// Why it does not read.
        why: String,
    },
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
            SecurityReason::Tampered => {
                write!(f, "is not what the build signed: the pack was changed")
            }
            SecurityReason::Malformed { expected, why } => {
                write!(f, "does not read as an {expected}: {why}")
            }
        }
    }
}

impl SecurityReason {
    /// Returns whether the file was refused for its size or a decoding limit, not for what it holds.
    ///
    /// Trusting a project lifts such a refusal: its caps are larger.
    pub fn is_size_or_limit(&self) -> bool {
        matches!(
            self,
            SecurityReason::TooLarge { .. } | SecurityReason::OverLimit { .. }
        )
    }

    /// Returns what to do about a file refused for this reason.
    pub fn remedy(&self) -> &'static str {
        match self {
            SecurityReason::Escape => {
                "Keep the file inside the project's folder: a path leaving it, or a link \
                 pointing out of it, is never read."
            }
            SecurityReason::TooLarge { .. } => {
                "Make the file smaller, or split it. A file from an untrusted project or \
                 downloaded from the internet has smaller caps."
            }
            SecurityReason::WrongType { .. } => {
                "Check the file: its extension does not match what it holds. Fix or \
                 replace it, and it is read again."
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
            SecurityReason::Tampered => {
                "Build the game again: a pack changed after its build is never read."
            }
            SecurityReason::Malformed { .. } => {
                "Mend the file: it does not read as what it is (a merge may have left its \
                 conflict markers in it). Once it changes, it is read again."
            }
        }
    }
}
