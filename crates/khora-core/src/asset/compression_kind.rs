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

//! The on-disk compression of a packed asset.

use serde::{Deserialize, Serialize};

/// On-disk compression scheme for a packed asset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionKind {
    /// Raw bytes — no compression applied. Fastest read path.
    #[default]
    None,
    /// LZ4 block compression (`lz4_flex`). Cheap to decompress, modest
    /// ratio. Good default for shipping builds.
    Lz4,
}
