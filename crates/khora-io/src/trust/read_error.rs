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

//! Why a guarded read returned no bytes.

use super::SecurityRefusal;

/// Why a [`FileGuard`](super::FileGuard) read returned no bytes.
#[derive(Debug)]
pub enum ReadError {
    /// The guard refused the file.
    Refused(SecurityRefusal),
    /// The file could not be read: missing, or the system refused it.
    Io(std::io::Error),
}

impl ReadError {
    /// Returns whether the file was not read because it does not exist.
    pub fn is_missing(&self) -> bool {
        matches!(self, ReadError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Refused(refusal) => write!(f, "refused: {refusal}"),
            ReadError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReadError {}
