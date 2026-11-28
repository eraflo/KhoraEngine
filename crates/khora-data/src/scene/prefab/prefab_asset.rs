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

//! [`PrefabAsset`], a prefab as the asset store holds it.

use khora_core::asset::{kinds, Asset, AssetKind, NamedAsset, NoLocalSource, Stored};

use crate::assets::AssetValueRegistration;
use crate::scene::SceneRecord;

/// A prefab file, read: the record of its members.
#[derive(Debug, Clone)]
pub struct PrefabAsset(pub SceneRecord);

impl Stored for PrefabAsset {}

impl Asset for PrefabAsset {
    const KIND: AssetKind = kinds::PREFAB;
    type LocalSource = NoLocalSource;

    /// A prefab lists no asset it needs.
    fn dependencies(&self, _: &mut Vec<NamedAsset>) {}
}

inventory::submit!(AssetValueRegistration::of::<PrefabAsset>());
