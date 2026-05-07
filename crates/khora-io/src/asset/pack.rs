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
//! `data.pack` files start with a 16-byte header so the loader can fail
//! fast on the wrong file (e.g. the user accidentally renamed something to
//! `data.pack`) and reject pack-format versions it wasn't built against.
//! See [`PACK_HEADER_SIZE`] and [`PackHeader`] for the byte layout, and
//! `crates/khora-io/src/asset/pack_builder.rs` for the writer side.

use anyhow::{bail, Context, Result};
use khora_core::asset::AssetSource;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

use super::AssetIo;

/// 8-byte magic prefix that identifies a Khora pack archive on disk.
///
/// Trailing NUL keeps it printable in `od -c` / hex dumps. Persisted as
/// raw bytes — endianness-independent.
pub const PACK_MAGIC: &[u8; 8] = b"KHORAPK\0";

/// Current pack format version. Bumped when the on-disk layout changes in
/// a non-additive way; older runtimes refuse newer packs by reading the
/// version from the header.
pub const PACK_FORMAT_VERSION: u32 = 1;

/// Total size in bytes of the leading [`PackHeader`] in `data.pack`.
/// Asset offsets recorded in `index.bin` are **relative to the start of
/// the asset region**, not to byte 0 — `PackLoader` adds this constant
/// when seeking.
pub const PACK_HEADER_SIZE: u64 = 16;

/// Decoded form of the 16-byte header at the start of `data.pack`.
///
/// Byte layout (all little-endian):
///
/// ```text
/// offset  size  field
/// ──────  ────  ─────────────────────────────────────────────────────
///      0     8  PACK_MAGIC (b"KHORAPK\0")
///      8     4  format_version: u32  — must equal PACK_FORMAT_VERSION
///     12     4  asset_count:    u32  — sanity check vs. index.bin
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PackHeader {
    /// Format version as written. Equals [`PACK_FORMAT_VERSION`] on a pack
    /// produced by this engine; loader refuses anything else.
    pub format_version: u32,
    /// Number of assets the producer claims to have written into this
    /// `data.pack`. Cross-checked against `index.bin.len()` at boot to
    /// catch a mismatched pair (e.g. user shipped only one of the two
    /// files).
    pub asset_count: u32,
}

impl PackHeader {
    /// Encodes the header into 16 raw bytes ready to be prepended to
    /// `data.pack`. Used by `PackBuilder`.
    pub fn to_bytes(&self) -> [u8; PACK_HEADER_SIZE as usize] {
        let mut out = [0u8; PACK_HEADER_SIZE as usize];
        out[0..8].copy_from_slice(PACK_MAGIC);
        out[8..12].copy_from_slice(&self.format_version.to_le_bytes());
        out[12..16].copy_from_slice(&self.asset_count.to_le_bytes());
        out
    }

    /// Convenience for the writer: build the v1 header for a pack that
    /// will contain `asset_count` blobs.
    pub fn v1(asset_count: u32) -> Self {
        Self {
            format_version: PACK_FORMAT_VERSION,
            asset_count,
        }
    }
}

/// Pack-based asset loader for release mode.
///
/// Reads assets from a `.pack` archive file by seeking to the recorded
/// offset (shifted by [`PACK_HEADER_SIZE`]) and reading the specified
/// number of bytes.
#[derive(Debug)]
pub struct PackLoader {
    pack_file: File,
    header: PackHeader,
}

impl PackLoader {
    /// Validates the leading header and returns a reader bound to
    /// `pack_file`.
    ///
    /// Errors:
    /// - file shorter than [`PACK_HEADER_SIZE`] or unreadable,
    /// - magic doesn't match [`PACK_MAGIC`] (file isn't a Khora pack),
    /// - `format_version` field doesn't match [`PACK_FORMAT_VERSION`]
    ///   (this runtime can't read this pack — typically an older runtime
    ///   reading a newer pack).
    pub fn new(mut pack_file: File) -> Result<Self> {
        let header =
            read_and_validate_header(&mut pack_file).context("Pack header validation failed")?;
        Ok(Self { pack_file, header })
    }

    /// Returns a reference to the parsed header. Useful for diagnostics
    /// and for asserting `header.asset_count == vfs.asset_count()` at
    /// startup.
    pub fn header(&self) -> &PackHeader {
        &self.header
    }

    /// Writes a fresh 16-byte header at the start of `out`. Convenience
    /// helper for [`crate::asset::PackBuilder`] — keeps all the byte-
    /// layout knowledge in one module.
    pub fn write_header(out: &mut impl Write, asset_count: u32) -> Result<()> {
        let header = PackHeader::v1(asset_count);
        out.write_all(&header.to_bytes())
            .context("Failed to write pack header")
    }
}

fn read_and_validate_header(file: &mut File) -> Result<PackHeader> {
    file.seek(SeekFrom::Start(0))
        .context("Failed to seek to pack file start")?;
    let mut buf = [0u8; PACK_HEADER_SIZE as usize];
    file.read_exact(&mut buf)
        .context("Pack file is shorter than 16 bytes — not a Khora pack (or truncated)")?;

    if &buf[0..8] != PACK_MAGIC {
        bail!(
            "Not a Khora pack archive (bad magic: expected {:?}, got {:?})",
            std::str::from_utf8(PACK_MAGIC).unwrap_or("KHORAPK\\0"),
            &buf[0..8]
        );
    }
    let format_version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
    if format_version != PACK_FORMAT_VERSION {
        bail!(
            "Unsupported pack format version {} (this runtime supports v{})",
            format_version,
            PACK_FORMAT_VERSION
        );
    }
    let asset_count = u32::from_le_bytes(buf[12..16].try_into().unwrap());
    Ok(PackHeader {
        format_version,
        asset_count,
    })
}

impl AssetIo for PackLoader {
    fn load_bytes(&mut self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Packed { offset, size } => {
                let mut buffer = vec![0; *size as usize];
                // Asset offsets in `index.bin` are relative to the start
                // of the asset region (i.e. after the header). We add the
                // header size here so the index never has to know about
                // the on-disk header.
                self.pack_file
                    .seek(SeekFrom::Start(PACK_HEADER_SIZE + *offset))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_pack(dir: &std::path::Path, bytes: &[u8]) -> File {
        let path = dir.join("data.pack");
        let mut f = File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        f.sync_all().unwrap();
        File::open(&path).unwrap()
    }

    fn valid_header_bytes(asset_count: u32) -> [u8; 16] {
        PackHeader::v1(asset_count).to_bytes()
    }

    #[test]
    fn rejects_file_with_wrong_magic() {
        let dir = tempdir().unwrap();
        let f = write_pack(dir.path(), b"not_a_pack......\x00\x00\x00\x00");
        let err = PackLoader::new(f).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("bad magic"), "got: {msg}");
    }

    #[test]
    fn rejects_file_shorter_than_header() {
        let dir = tempdir().unwrap();
        let f = write_pack(dir.path(), b"KHORAPK"); // 7 bytes, less than 16
        let err = PackLoader::new(f).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("shorter than 16 bytes"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempdir().unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(PACK_MAGIC); // 8
        bytes.extend_from_slice(&999u32.to_le_bytes()); // version
        bytes.extend_from_slice(&0u32.to_le_bytes()); // asset_count
        let f = write_pack(dir.path(), &bytes);
        let err = PackLoader::new(f).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Unsupported pack format version 999"),
            "got: {msg}"
        );
    }

    #[test]
    fn accepts_valid_header_and_exposes_asset_count() {
        let dir = tempdir().unwrap();
        let mut bytes = valid_header_bytes(42).to_vec();
        bytes.extend_from_slice(b"\xAA\xBB\xCC"); // some payload
        let f = write_pack(dir.path(), &bytes);
        let loader = PackLoader::new(f).unwrap();
        assert_eq!(loader.header().format_version, PACK_FORMAT_VERSION);
        assert_eq!(loader.header().asset_count, 42);
    }

    #[test]
    fn load_bytes_reads_relative_to_header() {
        let dir = tempdir().unwrap();
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(b"PAYLOAD");
        let f = write_pack(dir.path(), &bytes);
        let mut loader = PackLoader::new(f).unwrap();
        let got = loader
            .load_bytes(&AssetSource::Packed { offset: 0, size: 7 })
            .unwrap();
        assert_eq!(got, b"PAYLOAD");
    }
}
