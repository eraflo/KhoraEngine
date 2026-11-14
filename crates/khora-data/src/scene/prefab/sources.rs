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

//! Prefab sources that wrap another: one that reads each prefab once, one
//! that refuses the prefab being written.

use std::cell::RefCell;
use std::collections::HashMap;

use khora_core::asset::AssetUUID;

use super::super::scene_record::SceneRecord;
use super::PrefabSource;

pub(super) use super::except::Except;

/// A source that reads each prefab once: an expansion asks for the same
/// prefab under every instance of it, and at every level it nests.
pub(super) struct ReadOnce<'a> {
    source: &'a dyn PrefabSource,
    read: RefCell<HashMap<AssetUUID, Result<SceneRecord, String>>>,
}

impl<'a> ReadOnce<'a> {
    pub(super) fn new(source: &'a dyn PrefabSource) -> Self {
        Self {
            source,
            read: RefCell::new(HashMap::new()),
        }
    }
}

impl PrefabSource for ReadOnce<'_> {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        if let Some(read) = self.read.borrow().get(&id) {
            return read.clone();
        }
        let read = self.source.prefab(id);
        self.read.borrow_mut().insert(id, read.clone());
        read
    }
}
