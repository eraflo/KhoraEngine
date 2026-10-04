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

use khora_core::asset::AssetUUID;
use khora_macros::Component;

/// Marks the root of a prefab instance: the entities under it are the
/// prefab's, each known by the root's identity and its own id in the prefab.
///
/// Placed by the tools when a prefab is instantiated; a scene saves the
/// instance as this link and its overrides, not as the prefab's entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Component)]
#[component(domain = Spatial, provenance = ToolAuthored)]
pub struct PrefabInstance {
    /// The prefab this instance was made from.
    pub prefab: AssetUUID,
}
