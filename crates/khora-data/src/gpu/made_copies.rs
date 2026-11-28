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

//! [`MadeCopies`]: which CPU copy each GPU copy of a residency was made
//! from, and which copies the device refused.

use std::collections::{HashMap, HashSet};

use khora_core::asset::{AssetHandle, AssetUUID, Stored};

/// What a residency remembers of the GPU copies it made, per uuid.
///
/// A GPU copy is current while the store still holds the CPU copy it was
/// made from (the same handle, compared by identity): a copy stored again
/// under its uuid, the same value, needs no new GPU copy.
pub(crate) struct MadeCopies<S: Stored> {
    /// The CPU copy each GPU copy was made from.
    made_from: HashMap<AssetUUID, AssetHandle<S>>,
    /// The uuids whose GPU copy the device refused, tried again each run.
    refused: HashSet<AssetUUID>,
}

impl<S: Stored> Default for MadeCopies<S> {
    fn default() -> Self {
        Self {
            made_from: HashMap::new(),
            refused: HashSet::new(),
        }
    }
}

impl<S: Stored> MadeCopies<S> {
    /// Returns whether the GPU copy of `uuid` was made from `source`.
    pub(crate) fn is_made_from(&self, uuid: &AssetUUID, source: &AssetHandle<S>) -> bool {
        self.made_from.get(uuid) == Some(source)
    }

    /// Notes that the GPU copy of `uuid` was made from `source`.
    pub(crate) fn made(&mut self, uuid: AssetUUID, source: AssetHandle<S>) {
        self.refused.remove(&uuid);
        self.made_from.insert(uuid, source);
    }

    /// Notes that the device refused the GPU copy of `uuid`: it is tried again on the next run.
    pub(crate) fn refuse(&mut self, uuid: AssetUUID) {
        self.refused.insert(uuid);
    }

    /// Takes the uuids refused since the last call.
    pub(crate) fn take_refused(&mut self) -> HashSet<AssetUUID> {
        std::mem::take(&mut self.refused)
    }

    /// Forgets the copy `uuid` was made from, so the next run makes it again.
    pub(crate) fn forget(&mut self, uuid: &AssetUUID) {
        self.made_from.remove(uuid);
    }

    /// Returns every uuid this record remembers, some more than once.
    pub(crate) fn every_known(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.made_from.keys().chain(&self.refused).copied()
    }

    /// Forgets everything about `uuid`: its source and its refusal.
    pub(crate) fn drop_uuid(&mut self, uuid: &AssetUUID) {
        self.made_from.remove(uuid);
        self.refused.remove(uuid);
    }
}

#[cfg(test)]
mod tests {
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::renderer::api::gpu_scene::{create_plane, Mesh};

    use super::MadeCopies;

    #[test]
    fn a_copy_is_current_only_for_the_handle_it_was_made_from() {
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        let first = AssetHandle::new(create_plane(1.0, 0.0));
        let same_value = AssetHandle::new(create_plane(1.0, 0.0));
        let mut made = MadeCopies::<Mesh>::default();
        assert!(!made.is_made_from(&uuid, &first), "nothing made yet");

        made.made(uuid, first.clone());
        assert!(made.is_made_from(&uuid, &first));
        assert!(
            !made.is_made_from(&uuid, &same_value),
            "another handle is another copy, whatever its value"
        );

        made.forget(&uuid);
        assert!(!made.is_made_from(&uuid, &first), "forgotten: made again");
    }

    #[test]
    fn a_refused_uuid_is_taken_once_and_cleared_when_made() {
        let uuid = AssetUUID::new_v5("meshes/plane.obj");
        let mut made = MadeCopies::<Mesh>::default();
        made.refuse(uuid);
        assert_eq!(made.take_refused().len(), 1);
        assert!(made.take_refused().is_empty(), "taken once");

        made.refuse(uuid);
        made.made(uuid, AssetHandle::new(create_plane(1.0, 0.0)));
        assert!(made.take_refused().is_empty(), "made, no longer refused");

        made.refuse(uuid);
        assert!(
            made.every_known().any(|known| known == uuid),
            "a refused uuid is known"
        );
        made.drop_uuid(&uuid);
        assert!(made.take_refused().is_empty(), "dropped, no longer refused");
        assert_eq!(made.every_known().count(), 0, "dropped, no longer known");
    }
}
