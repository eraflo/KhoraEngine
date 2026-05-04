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

//! Pack-based asset loader for release mode.
//!
//! Reads assets from a `.pack` archive file by seeking to the recorded offset.

use anyhow::{bail, Context, Result};
use khora_core::asset::AssetSource;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

use super::AssetIo;

/// Pack-based asset loader for release mode.
///
/// Reads assets from a `.pack` archive file by seeking to the recorded offset
/// and reading the specified number of bytes.
pub struct PackLoader {
    pack_file: File,
}

impl PackLoader {
    /// Creates a new `PackLoader` with the given pack file handle.
    pub fn new(pack_file: File) -> Self {
        Self { pack_file }
    }
}

impl AssetIo for PackLoader {
    fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
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
                bail!("PackLoader does not support Path sources")
            }
        }
    }
}
