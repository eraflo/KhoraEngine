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

//! The header that starts `data.pack`: its magic, version and flags.

/// The 8-byte magic that starts a Khora pack archive.
pub const PACK_MAGIC: &[u8; 8] = b"KHORAPK\0";

/// The pack format version this runtime reads, and refuses any other.
///
/// Bumped when the layout changes in a way an older runtime cannot read.
pub const PACK_FORMAT_VERSION: u32 = 3;

/// The size in bytes of the [`PackHeader`] that starts `data.pack`.
///
/// Offsets in `index.bin` are relative to the asset region after it, not to byte 0.
pub const PACK_HEADER_SIZE: u64 = 24;

/// Header flag: at least one entry of the pack was written LZ4-compressed.
///
/// For diagnostics only: each entry's `AssetSource::Packed::compression` decides.
pub const PACK_FLAG_LZ4: u32 = 1 << 0;
/// Header flag: a `manifest.bin` accompanies the pack.
pub const PACK_FLAG_MANIFEST: u32 = 1 << 1;

/// The 24-byte header at the start of `data.pack`, decoded.
///
/// Byte layout (all little-endian):
///
/// ```text
/// offset  size  field
/// ──────  ────  ─────────────────────────────────────────────────────
///      0     8  PACK_MAGIC (b"KHORAPK\0")
///      8     4  format_version: u32  — must equal PACK_FORMAT_VERSION
///     12     4  asset_count:    u32  — index.bin's entry count
///     16     4  flags:          u32  — bit 0 LZ4, bit 1 manifest
///     20     4  reserved:       u32  — zero on writes, ignored on reads
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PackHeader {
    /// The format version as written; the loader refuses any but [`PACK_FORMAT_VERSION`].
    pub format_version: u32,
    /// The number of assets written into the pack.
    ///
    /// The entries of `index.bin`, sub-assets included; nothing checks it
    /// against the index yet.
    pub asset_count: u32,
    /// Pack-level flags: the `PACK_FLAG_*` constants.
    pub flags: u32,
}

impl PackHeader {
    /// Encodes the header into the 24 bytes that start `data.pack`.
    pub fn to_bytes(&self) -> [u8; PACK_HEADER_SIZE as usize] {
        let mut out = [0u8; PACK_HEADER_SIZE as usize];
        out[0..8].copy_from_slice(PACK_MAGIC);
        out[8..12].copy_from_slice(&self.format_version.to_le_bytes());
        out[12..16].copy_from_slice(&self.asset_count.to_le_bytes());
        out[16..20].copy_from_slice(&self.flags.to_le_bytes());
        // Bytes 20..24 reserved — left zero.
        out
    }

    /// Returns the current-version header of a pack of `asset_count` entries and `flags`.
    pub fn current(asset_count: u32, flags: u32) -> Self {
        Self {
            format_version: PACK_FORMAT_VERSION,
            asset_count,
            flags,
        }
    }
}
