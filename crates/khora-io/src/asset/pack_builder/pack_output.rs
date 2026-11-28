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

//! What a pack build wrote.

use std::path::PathBuf;

use super::super::LeftOutFile;

/// Output of a successful [`PackBuilder::build`](super::PackBuilder::build).
#[derive(Debug, Clone)]
pub struct PackOutput {
    /// Absolute path to the produced `index.bin`.
    pub index_bin: PathBuf,
    /// Absolute path to the produced `data.pack`.
    pub data_pack: PathBuf,
    /// Absolute path to the produced `manifest.bin`; `None` for a build without a signer.
    pub manifest_bin: Option<PathBuf>,
    /// Number of assets indexed (mirrors `index.bin`'s entry count).
    pub asset_count: usize,
    /// The external files left out of the pack, sorted by path: refused by the
    /// guard, over their container's cap, or missing.
    pub left_out: Vec<LeftOutFile>,
    /// The size of `data.pack`'s asset region in bytes, its header aside.
    pub(super) pack_bytes: u64,
}

impl PackOutput {
    /// Returns the size of `data.pack`'s asset region in bytes, its header aside.
    pub fn pack_bytes(&self) -> u64 {
        self.pack_bytes
    }
}
