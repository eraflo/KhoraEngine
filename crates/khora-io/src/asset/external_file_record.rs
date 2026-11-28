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

//! One row of a manifest's external-files records: a packed external file's integrity.

use serde::{Deserialize, Serialize};

/// The integrity record of a container's external file in a pack, by its path: it has no uuid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalFileRecord {
    /// Its forward-slash path under the assets root.
    pub path: String,
    /// BLAKE3 hash of its *uncompressed* bytes, as a 32-byte digest.
    pub blake3: [u8; 32],
    /// Uncompressed size in bytes, checked before the hash is computed.
    pub size: u64,
}
