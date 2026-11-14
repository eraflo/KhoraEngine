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

//! The asset loader of a release build, reading entries from a `data.pack`.
//!
//! The byte layout is [`PackHeader`]'s; `pack_builder.rs` writes it.

use anyhow::{bail, Context, Result};
use khora_core::asset::{AssetSource, CompressionKind};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

use super::AssetIo;
use crate::trust::LimitExceeded;

/// The 8-byte magic that starts a Khora pack archive.
pub const PACK_MAGIC: &[u8; 8] = b"KHORAPK\0";

/// The pack format version this runtime reads, and refuses any other.
///
/// Bumped when the layout changes in a way an older runtime cannot read.
pub const PACK_FORMAT_VERSION: u32 = 2;

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
    pub fn v2(asset_count: u32, flags: u32) -> Self {
        Self {
            format_version: PACK_FORMAT_VERSION,
            asset_count,
            flags,
        }
    }
}

/// The asset loader of a release build, reading entries from a `data.pack` file.
#[derive(Debug)]
pub struct PackLoader {
    pack_file: File,
    header: PackHeader,
}

impl PackLoader {
    /// Returns a loader reading `pack_file`, once its header is checked.
    ///
    /// # Errors
    /// When the file is unreadable or shorter than [`PACK_HEADER_SIZE`], its magic
    /// is not [`PACK_MAGIC`], or its version is not [`PACK_FORMAT_VERSION`].
    pub fn new(mut pack_file: File) -> Result<Self> {
        let header =
            read_and_validate_header(&mut pack_file).context("Pack header validation failed")?;
        Ok(Self { pack_file, header })
    }

    /// Returns the pack's header, as read.
    pub fn header(&self) -> &PackHeader {
        &self.header
    }

    /// Writes a current-version header of `asset_count` entries and `flags` to `out`.
    pub fn write_header(out: &mut impl Write, asset_count: u32, flags: u32) -> Result<()> {
        let header = PackHeader::v2(asset_count, flags);
        out.write_all(&header.to_bytes())
            .context("Failed to write pack header")
    }

    /// Reads exactly `size` bytes of the pack file from byte `at`, sharing no cursor.
    ///
    /// Several threads read one loader at once.
    fn read_at(&self, at: u64, size: u64) -> Result<Vec<u8>> {
        // An entry the file cannot hold is refused before anything is
        // allocated for it: a corrupt index must not ask for more memory
        // than the pack has bytes.
        let file_len = self
            .pack_file
            .metadata()
            .context("Failed to read the pack file's length")?
            .len();
        if at.checked_add(size).is_none_or(|end| end > file_len) {
            bail!(
                "Pack entry of {size} bytes at {at} runs past the end of the {file_len}-byte file"
            );
        }
        let len = usize::try_from(size).context("Pack entry is larger than memory")?;
        let mut buffer = vec![0u8; len];
        let mut filled = 0;
        while filled < len {
            let read = read_once(&self.pack_file, &mut buffer[filled..], at + filled as u64)
                .with_context(|| format!("Failed to read {size} bytes at {at} from pack"))?;
            if read == 0 {
                bail!(
                    "Pack entry of {size} bytes at {at} runs past the end of the file \
                     ({filled} bytes read)"
                );
            }
            filled += read;
        }
        Ok(buffer)
    }
}

/// The most bytes one pack entry may hold, compressed or not.
const MAX_ENTRY_BYTES: u64 = 4 << 30;

/// The most LZ4 can expand one compressed byte to.
const LZ4_MAX_RATIO: u64 = 255;
/// Room for the end of an LZ4 block, which carries a few literals.
const LZ4_SLACK: u64 = 64;

/// Reads `file` into `buf` at `offset`, without a cursor shared with other readers.
///
/// Windows moves the handle's own position, never relied on here.
#[cfg(windows)]
fn read_once(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

#[cfg(unix)]
fn read_once(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(not(any(windows, unix)))]
fn read_once(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    let _ = (file, buf, offset);
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "positioned reads are not supported on this platform",
    ))
}

fn read_and_validate_header(file: &mut File) -> Result<PackHeader> {
    file.seek(SeekFrom::Start(0))
        .context("Failed to seek to pack file start")?;
    let mut buf = [0u8; PACK_HEADER_SIZE as usize];
    file.read_exact(&mut buf)
        .context("Pack file is shorter than 24 bytes — not a Khora pack (or truncated)")?;

    if &buf[0..8] != PACK_MAGIC {
        bail!(
            "Not a Khora pack archive (bad magic: expected {:?}, got {:?})",
            std::str::from_utf8(PACK_MAGIC).unwrap_or("KHORAPK\\0"),
            &buf[0..8]
        );
    }
    // `buf` is exactly `PACK_HEADER_SIZE` bytes and `read_exact` succeeded, so
    // each 4-byte window below is in bounds; propagate rather than unwrap so a
    // future header-size change surfaces as an error instead of a panic.
    let to_u32 = |slice: &[u8]| -> Result<u32> {
        let arr: [u8; 4] = slice
            .try_into()
            .context("Pack header field is not 4 bytes — truncated or corrupt pack")?;
        Ok(u32::from_le_bytes(arr))
    };
    let format_version = to_u32(&buf[8..12])?;
    if format_version != PACK_FORMAT_VERSION {
        bail!(
            "Unsupported pack format version {} (this runtime supports v{})",
            format_version,
            PACK_FORMAT_VERSION
        );
    }
    let asset_count = to_u32(&buf[12..16])?;
    let flags = to_u32(&buf[16..20])?;
    Ok(PackHeader {
        format_version,
        asset_count,
        flags,
    })
}

impl AssetIo for PackLoader {
    fn load_bytes(&self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Packed {
                offset,
                size,
                uncompressed_size,
                compression,
            } => {
                // Whatever the reader checked: no entry is ever allocated
                // past this.
                for (what, bytes) in [
                    ("pack entry", *size),
                    ("its decompressed bytes", *uncompressed_size),
                ] {
                    if bytes > MAX_ENTRY_BYTES {
                        return Err(anyhow::Error::new(LimitExceeded {
                            what: format!("{what}, {bytes} bytes"),
                            limit: MAX_ENTRY_BYTES,
                        }));
                    }
                }
                // Offsets in `index.bin` are relative to the asset region, so
                // the index never knows of the header.
                let at = PACK_HEADER_SIZE.checked_add(*offset).ok_or_else(|| {
                    anyhow::anyhow!("Pack entry offset {offset} is past any pack file")
                })?;
                let buffer = self.read_at(at, *size)?;

                match compression {
                    CompressionKind::None => Ok(buffer),
                    CompressionKind::Lz4 => {
                        // LZ4 expands a byte to at most 255: a larger claim
                        // is a corrupt index, refused before it is allocated.
                        let bound = size.saturating_mul(LZ4_MAX_RATIO).saturating_add(LZ4_SLACK);
                        if *uncompressed_size > bound {
                            bail!(
                                "Pack: LZ4 entry of {size} bytes claims {uncompressed_size} \
                                 bytes uncompressed, more than LZ4 can produce"
                            );
                        }
                        let uncompressed = usize::try_from(*uncompressed_size)
                            .context("Pack entry is larger than memory")?;
                        let decompressed = lz4_flex::block::decompress(&buffer, uncompressed)
                            .map_err(|e| anyhow::anyhow!("LZ4 decompress failed: {}", e))?;
                        if decompressed.len() != uncompressed {
                            bail!(
                                "Pack: LZ4-decompressed size {} != recorded uncompressed_size {}",
                                decompressed.len(),
                                uncompressed_size
                            );
                        }
                        Ok(decompressed)
                    }
                }
            }
            AssetSource::Path(_) => {
                bail!("PackLoader does not support Path sources")
            }
            AssetSource::Sub { container, label } => {
                bail!("PackLoader: sub-asset '{label}' of {container} has no bytes of its own")
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

    fn valid_header_bytes(asset_count: u32) -> [u8; PACK_HEADER_SIZE as usize] {
        PackHeader::v2(asset_count, 0).to_bytes()
    }

    #[test]
    fn rejects_file_with_wrong_magic() {
        let dir = tempdir().unwrap();
        let f = write_pack(
            dir.path(),
            b"not_a_pack......\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        );
        let err = PackLoader::new(f).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("bad magic"), "got: {msg}");
    }

    #[test]
    fn rejects_file_shorter_than_header() {
        let dir = tempdir().unwrap();
        let f = write_pack(dir.path(), b"KHORAPK"); // 7 bytes, less than header
        let err = PackLoader::new(f).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("shorter than 24 bytes"), "got: {msg}");
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempdir().unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(PACK_MAGIC); // 8
        bytes.extend_from_slice(&999u32.to_le_bytes()); // version
        bytes.extend_from_slice(&0u32.to_le_bytes()); // asset_count
        bytes.extend_from_slice(&0u32.to_le_bytes()); // flags
        bytes.extend_from_slice(&0u32.to_le_bytes()); // reserved
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
        let loader = PackLoader::new(f).unwrap();
        let got = loader
            .load_bytes(&AssetSource::Packed {
                offset: 0,
                size: 7,
                uncompressed_size: 7,
                compression: CompressionKind::None,
            })
            .unwrap();
        assert_eq!(got, b"PAYLOAD");
    }

    #[test]
    fn load_bytes_decompresses_lz4() {
        let dir = tempdir().unwrap();
        let payload = b"PAYLOAD-ALL-COMPRESSIBLE-PAYLOAD-ALL-COMPRESSIBLE";
        let compressed = lz4_flex::block::compress(payload);
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(&compressed);
        let f = write_pack(dir.path(), &bytes);
        let loader = PackLoader::new(f).unwrap();
        let got = loader
            .load_bytes(&AssetSource::Packed {
                offset: 0,
                size: compressed.len() as u64,
                uncompressed_size: payload.len() as u64,
                compression: CompressionKind::Lz4,
            })
            .unwrap();
        assert_eq!(got, payload);
    }

    /// A sub-asset has no packed bytes of its own; the service reads its
    /// container's packed entry.
    #[test]
    fn load_bytes_refuses_a_sub_source() {
        let dir = tempdir().unwrap();
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(b"PAYLOAD");
        let f = write_pack(dir.path(), &bytes);
        let loader = PackLoader::new(f).unwrap();
        let result = loader.load_bytes(&AssetSource::Sub {
            container: khora_core::asset::AssetUUID::new_v5("models/hero.glb"),
            label: "mesh/Body".to_string(),
        });
        assert!(result.is_err(), "a Sub source has no packed bytes");
    }

    /// Reads from many threads at once never mix: each returns its own entry's bytes.
    #[test]
    fn a_pack_read_from_many_threads_at_once_returns_each_entry_exactly() {
        const ENTRIES: usize = 24;
        const THREADS: usize = 8;
        const READS: usize = 400;

        let payloads: Vec<Vec<u8>> = (0..ENTRIES)
            .map(|i| {
                (0..(97 + i * 53))
                    .map(|j| ((i * 31 + j * 7) % 251) as u8)
                    .collect()
            })
            .collect();
        let mut bytes = valid_header_bytes(ENTRIES as u32).to_vec();
        let mut sources = Vec::with_capacity(ENTRIES);
        for (i, payload) in payloads.iter().enumerate() {
            let offset = (bytes.len() as u64) - PACK_HEADER_SIZE;
            if i % 3 == 0 {
                let compressed = lz4_flex::block::compress(payload);
                bytes.extend_from_slice(&compressed);
                sources.push(AssetSource::Packed {
                    offset,
                    size: compressed.len() as u64,
                    uncompressed_size: payload.len() as u64,
                    compression: CompressionKind::Lz4,
                });
            } else {
                bytes.extend_from_slice(payload);
                sources.push(AssetSource::Packed {
                    offset,
                    size: payload.len() as u64,
                    uncompressed_size: payload.len() as u64,
                    compression: CompressionKind::None,
                });
            }
        }
        let dir = tempdir().unwrap();
        let loader = PackLoader::new(write_pack(dir.path(), &bytes)).unwrap();

        std::thread::scope(|scope| {
            for thread in 0..THREADS {
                let (loader, sources, payloads) = (&loader, &sources, &payloads);
                scope.spawn(move || {
                    for read in 0..READS {
                        let i = (thread * 5 + read * 3) % ENTRIES;
                        let got = loader
                            .load_bytes(&sources[i])
                            .unwrap_or_else(|e| panic!("entry {i} reads: {e:#}"));
                        assert_eq!(got, payloads[i], "entry {i}, thread {thread}, read {read}");
                    }
                });
            }
        });
    }

    /// Reading leaves nothing behind for the next read: the same entry
    /// read after others is the same bytes.
    #[test]
    fn a_pack_entry_read_again_after_others_returns_the_same_bytes() {
        let dir = tempdir().unwrap();
        let mut bytes = valid_header_bytes(2).to_vec();
        bytes.extend_from_slice(b"FIRST-ENTRY");
        bytes.extend_from_slice(b"SECOND");
        let loader = PackLoader::new(write_pack(dir.path(), &bytes)).unwrap();
        let first = AssetSource::Packed {
            offset: 0,
            size: 11,
            uncompressed_size: 11,
            compression: CompressionKind::None,
        };
        let second = AssetSource::Packed {
            offset: 11,
            size: 6,
            uncompressed_size: 6,
            compression: CompressionKind::None,
        };

        assert_eq!(loader.load_bytes(&second).unwrap(), b"SECOND");
        assert_eq!(loader.load_bytes(&first).unwrap(), b"FIRST-ENTRY");
        assert_eq!(loader.load_bytes(&second).unwrap(), b"SECOND");
        assert_eq!(loader.load_bytes(&first).unwrap(), b"FIRST-ENTRY");
    }

    /// An entry recorded past the end of the pack is an error, not a short
    /// or zero-filled read.
    #[test]
    fn a_pack_entry_past_the_end_of_the_file_is_an_error() {
        let dir = tempdir().unwrap();
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(b"SHORT");
        let loader = PackLoader::new(write_pack(dir.path(), &bytes)).unwrap();
        let result = loader.load_bytes(&AssetSource::Packed {
            offset: 2,
            size: 10,
            uncompressed_size: 10,
            compression: CompressionKind::None,
        });
        assert!(result.is_err(), "a truncated entry is refused: {result:?}");
    }

    /// The pack holding one 7-byte `PAYLOAD` entry, as a loader.
    fn one_entry_loader(dir: &std::path::Path) -> PackLoader {
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(b"PAYLOAD");
        PackLoader::new(write_pack(dir, &bytes)).unwrap()
    }

    /// A corrupt index can record any size: an entry no memory could hold is
    /// refused as an error, never a panic of the thread decoding it.
    #[test]
    fn a_pack_entry_larger_than_any_memory_is_an_error_not_a_panic() {
        let dir = tempdir().unwrap();
        let loader = one_entry_loader(dir.path());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loader.load_bytes(&AssetSource::Packed {
                offset: 0,
                size: u64::MAX,
                uncompressed_size: u64::MAX,
                compression: CompressionKind::None,
            })
        }));
        match result {
            Ok(read) => assert!(read.is_err(), "an impossible size is refused: {read:?}"),
            Err(_) => panic!("an entry of u64::MAX bytes panics the read"),
        }
    }

    /// A corrupt index can record any offset: one whose end lies past every
    /// file is an error, never an overflow panic.
    #[test]
    fn a_pack_entry_at_an_offset_past_any_file_is_an_error_not_a_panic() {
        let dir = tempdir().unwrap();
        let loader = one_entry_loader(dir.path());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loader.load_bytes(&AssetSource::Packed {
                offset: u64::MAX - 4,
                size: 7,
                uncompressed_size: 7,
                compression: CompressionKind::None,
            })
        }));
        match result {
            Ok(read) => assert!(read.is_err(), "an impossible offset is refused: {read:?}"),
            Err(_) => panic!("an entry at offset u64::MAX - 4 panics the read"),
        }
    }

    /// A compressed entry recording an uncompressed size no memory could hold
    /// is refused as an error, never a panic.
    #[test]
    fn a_compressed_pack_entry_claiming_an_impossible_size_is_an_error_not_a_panic() {
        let dir = tempdir().unwrap();
        let compressed = lz4_flex::block::compress(b"PAYLOAD");
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(&compressed);
        let loader = PackLoader::new(write_pack(dir.path(), &bytes)).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loader.load_bytes(&AssetSource::Packed {
                offset: 0,
                size: compressed.len() as u64,
                uncompressed_size: u64::MAX,
                compression: CompressionKind::Lz4,
            })
        }));
        match result {
            Ok(read) => assert!(read.is_err(), "an impossible size is refused: {read:?}"),
            Err(_) => panic!("an LZ4 entry claiming u64::MAX bytes panics the read"),
        }
    }

    /// **A pack entry over 4 GiB is refused before anything is allocated.**
    ///
    /// A raw entry of 4 GiB and one byte, and a compressed one claiming as much,
    /// give a `LimitExceeded` naming the 4 GiB, unread.
    #[test]
    fn a_pack_entry_over_four_gib_is_refused_before_allocating() {
        use crate::trust::LimitExceeded;
        const FOUR_GIB: u64 = 4 << 30;

        let dir = tempdir().unwrap();
        let compressed = lz4_flex::block::compress(b"PAYLOAD");
        let mut bytes = valid_header_bytes(1).to_vec();
        bytes.extend_from_slice(&compressed);
        let loader = PackLoader::new(write_pack(dir.path(), &bytes)).unwrap();

        for (what, source) in [
            (
                "raw",
                AssetSource::Packed {
                    offset: 0,
                    size: FOUR_GIB + 1,
                    uncompressed_size: FOUR_GIB + 1,
                    compression: CompressionKind::None,
                },
            ),
            (
                "compressed",
                AssetSource::Packed {
                    offset: 0,
                    size: compressed.len() as u64,
                    uncompressed_size: FOUR_GIB + 1,
                    compression: CompressionKind::Lz4,
                },
            ),
        ] {
            let error = loader
                .load_bytes(&source)
                .expect_err("an entry over 4 GiB is refused");
            let limit = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<LimitExceeded>())
                .unwrap_or_else(|| panic!("{what}: a LimitExceeded, not: {error:#}"));
            assert_eq!(limit.limit, FOUR_GIB, "{what}");
        }
    }

    /// An entry of no bytes reads as no bytes, wherever it is recorded.
    #[test]
    fn a_zero_size_pack_entry_reads_as_no_bytes() {
        let dir = tempdir().unwrap();
        let loader = one_entry_loader(dir.path());
        for offset in [0, 7] {
            let got = loader
                .load_bytes(&AssetSource::Packed {
                    offset,
                    size: 0,
                    uncompressed_size: 0,
                    compression: CompressionKind::None,
                })
                .unwrap();
            assert!(got.is_empty(), "offset {offset}: {got:?}");
        }
    }

    /// The last entry of the pack, ending exactly at the end of the file,
    /// reads whole.
    #[test]
    fn a_pack_entry_ending_at_the_end_of_the_file_reads_whole() {
        let dir = tempdir().unwrap();
        let loader = one_entry_loader(dir.path());
        let got = loader
            .load_bytes(&AssetSource::Packed {
                offset: 3,
                size: 4,
                uncompressed_size: 4,
                compression: CompressionKind::None,
            })
            .unwrap();
        assert_eq!(got, b"LOAD");
    }
}
