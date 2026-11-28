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

//! What the registry's file held when the registry last read or wrote it.

/// What the registry's file held when the registry last read or wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stamp {
    /// Nothing is known of it: it was refused or could not be read.
    Unknown,
    /// There was no file.
    Missing,
    /// The file held bytes of this blake3 hash.
    Content([u8; 32]),
}

impl Stamp {
    /// Returns the stamp of a file read as `bytes`; `None` for a missing one.
    pub(super) fn of(bytes: Option<&[u8]>) -> Self {
        match bytes {
            None => Self::Missing,
            Some(bytes) => Self::Content(*blake3::hash(bytes).as_bytes()),
        }
    }
}
