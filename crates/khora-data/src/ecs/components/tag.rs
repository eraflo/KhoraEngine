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

use khora_macros::Component;
use std::collections::BTreeSet;

/// Free-form string tags attached to an entity for grouping and querying.
///
/// `BTreeSet` (rather than `HashSet`) gives a stable iteration order, which
/// the inspector relies on to render chips deterministically and the
/// serializer relies on for byte-for-byte stable output. Tags are
/// case-sensitive on storage; query helpers in
/// [`crate::ecs::world_ext`] do exact matching.
///
/// Note: this is the **entity-side** tag system. There is also
/// [`khora_core::asset::AssetMetadata::tags`] for asset-side tags — they
/// are not unified and serve different domains.
#[derive(Debug, Clone, PartialEq, Eq, Component, Default)]
pub struct Tag(pub BTreeSet<String>);

impl Tag {
    /// Creates an empty `Tag` set.
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Adds a tag. Returns `true` if it was newly inserted.
    pub fn insert(&mut self, tag: impl Into<String>) -> bool {
        self.0.insert(tag.into())
    }

    /// Removes a tag. Returns `true` if it was present.
    pub fn remove(&mut self, tag: &str) -> bool {
        self.0.remove(tag)
    }

    /// Returns `true` when the entity carries the given tag.
    pub fn contains(&self, tag: &str) -> bool {
        self.0.contains(tag)
    }

    /// Returns the tag count.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` when the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates the tags in lexicographic order.
    pub fn iter(&self) -> std::collections::btree_set::Iter<'_, String> {
        self.0.iter()
    }
}

impl<S: Into<String>> FromIterator<S> for Tag {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self(iter.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_dedup_and_iter_order() {
        let mut t = Tag::new();
        assert!(t.insert("zeta"));
        assert!(t.insert("alpha"));
        assert!(!t.insert("alpha"));
        let collected: Vec<&str> = t.iter().map(String::as_str).collect();
        assert_eq!(collected, vec!["alpha", "zeta"]);
        assert!(t.contains("alpha"));
        assert!(!t.contains("beta"));
    }

    #[test]
    fn from_iter_constructs_set() {
        let t = Tag::from_iter(["enemy", "boss", "enemy"]);
        assert_eq!(t.len(), 2);
    }
}
