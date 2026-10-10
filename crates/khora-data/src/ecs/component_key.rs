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

//! The key storage knows a component by.

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use crate::ecs::AnyVec;

/// What identifies a component to storage — without any `World`.
///
/// A page's columns are keyed by it, so a page describes itself: reading a
/// column needs the page and the key, never the world that filled it, and a
/// component has the same key in every world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentKey {
    /// A Rust component: its `TypeId`, a compile-time constant.
    Rust(TypeId),
    /// A component declared while the engine runs (by a script): a stable
    /// 128-bit hash of its name — the same in every world, every process and
    /// every run.
    Declared(u128),
}

impl ComponentKey {
    /// The key of the Rust component `T`.
    pub fn of<T: 'static>() -> Self {
        Self::Rust(TypeId::of::<T>())
    }

    /// The key of the component declared under `name`.
    pub const fn named(name: &str) -> Self {
        Self::Declared(fnv1a_128(name.as_bytes()))
    }
}

/// A key is hashed as the one 64-bit word it already is a hash of — a
/// `TypeId` hashes its own id, a declared key is FNV — so a page finds a
/// column with one cheap step per row (see [`KeyHasher`]). Keys of the two
/// kinds may share a hash; equality tells them apart.
impl Hash for ComponentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Rust(type_id) => type_id.hash(state),
            Self::Declared(hash) => state.write_u64(*hash as u64),
        }
    }
}

/// The hasher of a map keyed by [`ComponentKey`]: it keeps the key's own hash
/// instead of hashing it again. A key is already a well-mixed hash, and a
/// page looks a column up on every row a query visits — a second hash there
/// is pure cost.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write_u64(&mut self, word: u64) {
        // Folded rather than replaced, so a key hashed as several words
        // still depends on all of them.
        self.0 = self.0.rotate_left(29) ^ word;
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }
}

/// A page's columns, by key, hashed with [`KeyHasher`].
pub type ColumnMap = HashMap<ComponentKey, Box<dyn AnyVec>, BuildHasherDefault<KeyHasher>>;

/// 128-bit FNV-1a: stable across builds and platforms, computable in a
/// `const`, and wide enough that two names sharing a hash is not a practical
/// concern — a registry still refuses one, should it happen.
const fn fnv1a_128(bytes: &[u8]) -> u128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    let mut at = 0;
    while at < bytes.len() {
        hash ^= bytes[at] as u128;
        hash = hash.wrapping_mul(PRIME);
        at += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The empty input is the offset basis; one byte is the published
    /// FNV-1a 128 value — pins the hash, so a declared key never changes.
    #[test]
    fn the_name_hash_is_fnv1a_128() {
        assert_eq!(fnv1a_128(b""), 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d);
        assert_eq!(fnv1a_128(b"a"), 0xd228_cb69_6f1a_8caf_7891_2b70_4e4a_8964);
    }
}
