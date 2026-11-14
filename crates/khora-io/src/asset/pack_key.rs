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

//! The pack key stamped into a runtime binary: a slot of a 32-byte magic
//! then 32 bytes of key, zero until the build stamps it.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::signing::PackKey;

/// The slot: a 32-byte magic, then 32 bytes of key, zero until stamped.
#[used]
static PACK_KEY_SLOT: [u8; 64] = slot(*b"KHORA PACK KEY SLOT, ED25519 v1:");

/// A slot holding `magic` and no key.
const fn slot(magic: [u8; 32]) -> [u8; 64] {
    let mut slot = [0u8; 64];
    let mut i = 0;
    while i < 32 {
        slot[i] = magic[i];
        i += 1;
    }
    slot
}

/// Returns the slot as the binary file holds it, stamp included.
///
/// Read volatile: a stamp writes the bytes after the build, so neither the
/// compiler nor the linker may assume the ones it was built with.
fn slot_bytes() -> [u8; 64] {
    // SAFETY: `PACK_KEY_SLOT` is a `'static`, initialised, properly aligned
    // `[u8; 64]` that nothing in the program writes; a volatile read of it
    // reads the bytes the binary file holds, a stamp's included.
    unsafe { std::ptr::read_volatile(&PACK_KEY_SLOT) }
}

/// Returns the slot's magic, read from the slot so the binary holds no second copy of it.
pub fn slot_magic() -> [u8; 32] {
    let mut magic = [0u8; 32];
    magic.copy_from_slice(&slot_bytes()[..32]);
    magic
}

/// Returns the key stamped into the running binary; `None` while its slot is zero.
pub fn stamped_pack_key() -> Option<PackKey> {
    let mut key = [0u8; 32];
    key.copy_from_slice(&slot_bytes()[32..]);
    (key != [0u8; 32]).then_some(PackKey(key))
}

/// Writes `key` into the pack key slot of the binary at `exe`, after its magic.
///
/// # Errors
/// When the magic is found zero or several times, the file left as it was, or
/// when the file cannot be read or written.
pub fn stamp_pack_key(exe: &Path, key: &PackKey) -> Result<()> {
    let magic = slot_magic();
    let bytes = std::fs::read(exe).with_context(|| format!("cannot read {}", exe.display()))?;
    let mut found = bytes
        .windows(magic.len())
        .enumerate()
        .filter(|(_, window)| *window == magic)
        .map(|(at, _)| at);
    let at = match (found.next(), found.next()) {
        (Some(at), None) => at,
        (None, _) => {
            return Err(anyhow!(
                "{} holds no pack key slot: it is not a Khora runtime",
                exe.display()
            ))
        }
        (Some(_), Some(_)) => {
            return Err(anyhow!(
                "{} holds several pack key slots: which one the runtime reads is unknown",
                exe.display()
            ))
        }
    };
    if at + 64 > bytes.len() {
        return Err(anyhow!("{} ends inside its pack key slot", exe.display()));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(exe)
        .with_context(|| format!("cannot write {}", exe.display()))?;
    file.seek(SeekFrom::Start((at + magic.len()) as u64))
        .and_then(|_| file.write_all(&key.0))
        .and_then(|()| file.sync_all())
        .with_context(|| format!("cannot stamp {}", exe.display()))?;
    Ok(())
}
