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

//! Appending entries to the asset region of `data.pack`.

use anyhow::{Context, Result};
use khora_core::asset::CompressionKind;
use std::{fs::File, io::Write};

use super::super::PackedFile;

/// Appends entries to the asset region of `data.pack`, each compressed when that makes it smaller.
pub(super) struct EntryWriter<'a> {
    /// The pack file, its header written.
    pub(super) file: &'a mut File,
    /// The compression the build asks for.
    pub(super) compression: CompressionKind,
    /// Where the next entry starts, from the end of the header.
    pub(super) offset: u64,
    /// Whether an entry was written LZ4-compressed.
    pub(super) any_lz4: bool,
}

impl EntryWriter<'_> {
    /// Appends `raw`, the bytes of the file at `path`; returns where they are.
    pub(super) fn write(&mut self, path: &str, raw: Vec<u8>) -> Result<PackedFile> {
        let uncompressed_size = raw.len() as u64;
        let (bytes, compression) = match self.compression {
            CompressionKind::None => (raw, CompressionKind::None),
            CompressionKind::Lz4 => {
                let compressed = lz4_flex::block::compress(&raw);
                // Skip compression when it makes the entry larger
                // (already-compressed media: PNG, OGG, FBX).
                if compressed.len() < raw.len() {
                    (compressed, CompressionKind::Lz4)
                } else {
                    (raw, CompressionKind::None)
                }
            }
        };
        self.any_lz4 |= compression == CompressionKind::Lz4;
        self.file
            .write_all(&bytes)
            .with_context(|| format!("Failed to append {path} to data.pack"))?;
        let written = PackedFile {
            path: path.to_owned(),
            offset: self.offset,
            size: bytes.len() as u64,
            uncompressed_size,
            compression,
        };
        self.offset += written.size;
        Ok(written)
    }
}
