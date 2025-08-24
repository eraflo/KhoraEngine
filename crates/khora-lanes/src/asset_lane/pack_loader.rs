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

use anyhow::{bail, Context, Result};
use khora_core::asset::AssetSource;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

/// A "Lane" responsible for the I/O task of reading raw asset data from a `data.pack` file.
///
/// This struct encapsulates the low-level logic of seeking to a specific
/// location in the pack file and reading the correct number of bytes.
pub struct PackLoadingLane {
    /// An open file handle to the `data.pack` file.
    pack_file: File,
}

impl PackLoadingLane {
    /// Creates a new lane with a handle to the pack file.
    pub fn new(pack_file: File) -> Self {
        Self { pack_file }
    }

    /// Reads the raw bytes of an asset from the pack file based on its location.
    pub fn load_asset_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Packed { offset, size } => {
                let mut buffer = vec![0; *size as usize];
                self.pack_file
                    .seek(SeekFrom::Start(*offset))
                    .context("Failed to seek to asset location in pack file")?;
                self.pack_file
                    .read_exact(&mut buffer)
                    .context("Failed to read asset bytes from pack file")?;
                Ok(buffer)
            }
            AssetSource::Path(_) => {
                bail!("PackLoadingLane cannot load assets from a Path source.")
            }
        }
    }
}
