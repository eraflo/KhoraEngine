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

//! [`ContentHasher`], the content of a value fed field by field into a stable uuid.

use uuid::Uuid;

use super::AssetUUID;
use crate::math::LinearRgba;

/// The namespace of every content uuid, so none is ever a path's or a sub-asset's.
const CONTENT_NAMESPACE: Uuid = Uuid::from_u128(0x6b68_6f72_615f_636f_6e74_656e_745f_7631);

/// The tag before a string.
const STR: u8 = 1;
/// The tag before an `f32`.
const F32: u8 = 2;
/// The tag before a `u32`.
const U32: u8 = 3;
/// The tag before a `bool`.
const BOOL: u8 = 4;
/// The tag before an asset slot, set or not.
const ASSET: u8 = 5;

/// The content of a value, fed field by field, named by one stable uuid.
///
/// Equal content fed in the same order gives the same uuid on every run and
/// every platform: the uuid is a name-based (SHA-1) uuid of the bytes fed. Each
/// write is tagged and framed, so two different sequences of writes never feed
/// the same bytes. Floats are fed by their bits: `-0.0` is not `0.0`.
#[derive(Debug, Clone, Default)]
pub struct ContentHasher {
    bytes: Vec<u8>,
}

impl ContentHasher {
    /// Returns a hasher fed nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds `text`, framed by its length.
    pub fn write_str(&mut self, text: &str) {
        self.bytes.push(STR);
        self.bytes
            .extend_from_slice(&(text.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(text.as_bytes());
    }

    /// Feeds `value` by its bits.
    pub fn write_f32(&mut self, value: f32) {
        self.bytes.push(F32);
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    /// Feeds `value`.
    pub fn write_u32(&mut self, value: u32) {
        self.bytes.push(U32);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Feeds `value`.
    pub fn write_bool(&mut self, value: bool) {
        self.bytes.push(BOOL);
        self.bytes.push(u8::from(value));
    }

    /// Feeds the four channels of `color`, each by its bits.
    pub fn write_rgba(&mut self, color: LinearRgba) {
        for channel in [color.r, color.g, color.b, color.a] {
            self.write_f32(channel);
        }
    }

    /// Feeds an asset slot: the asset it names, or that it names none.
    pub fn write_asset(&mut self, asset: Option<AssetUUID>) {
        self.bytes.push(ASSET);
        match asset {
            Some(uuid) => {
                self.bytes.push(1);
                self.bytes.extend_from_slice(uuid.as_bytes());
            }
            None => self.bytes.push(0),
        }
    }

    /// Returns the uuid of everything fed so far.
    pub fn uuid(&self) -> AssetUUID {
        AssetUUID::from_bytes(*Uuid::new_v5(&CONTENT_NAMESPACE, &self.bytes).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same writes give the same uuid; a different value another.
    #[test]
    fn equal_content_gives_one_uuid() {
        let fed = |value: f32| {
            let mut hasher = ContentHasher::new();
            hasher.write_str("Thing");
            hasher.write_f32(value);
            hasher.uuid()
        };
        assert_eq!(fed(1.0), fed(1.0));
        assert_ne!(fed(1.0), fed(2.0));
        assert_ne!(fed(0.0), fed(-0.0), "floats are fed by their bits");
    }

    /// The uuid is pinned: it never moves between runs, builds or platforms.
    #[test]
    fn the_uuid_of_a_content_is_stable() {
        let mut hasher = ContentHasher::new();
        hasher.write_str("Thing");
        hasher.write_u32(7);
        hasher.write_bool(true);
        hasher.write_rgba(LinearRgba::new(1.0, 0.5, 0.25, 1.0));
        hasher.write_asset(Some(AssetUUID::from_bytes([3; 16])));
        hasher.write_asset(None);
        assert_eq!(
            hasher.uuid().to_string(),
            "ff25dd0a-6cea-586e-bd27-ed5304d8ece4"
        );
    }

    /// Framed writes never run together: `"ab" "c"` is not `"a" "bc"`, and a
    /// `u32` is not the `f32` of the same bits.
    #[test]
    fn different_writes_never_feed_the_same_bytes() {
        let strings = |a: &str, b: &str| {
            let mut hasher = ContentHasher::new();
            hasher.write_str(a);
            hasher.write_str(b);
            hasher.uuid()
        };
        assert_ne!(strings("ab", "c"), strings("a", "bc"));

        let mut as_u32 = ContentHasher::new();
        as_u32.write_u32(1.0f32.to_bits());
        let mut as_f32 = ContentHasher::new();
        as_f32.write_f32(1.0);
        assert_ne!(as_u32.uuid(), as_f32.uuid());

        let mut none = ContentHasher::new();
        none.write_asset(None);
        let mut nothing = ContentHasher::new();
        nothing.write_bool(false);
        assert_ne!(none.uuid(), nothing.uuid());
    }

    /// A content uuid is never the uuid of a path of the same bytes.
    #[test]
    fn a_content_uuid_is_no_path_uuid() {
        let hasher = ContentHasher::new();
        assert_ne!(hasher.uuid(), AssetUUID::new_v5(""));
    }
}
