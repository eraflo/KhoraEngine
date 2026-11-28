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

//! The scene's environment source for the IBL bake.

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::resource::CpuTexture;

use crate::ecs::AssetRef;

/// Selects the scene's environment source for the IBL bake.
///
/// A shared resource set by the host. It names an equirectangular texture (an
/// HDR keeps the range reflections need); absent or not yet loaded, the
/// procedural sky is baked instead. Selecting another texture, or the texture
/// changing in the store, bakes again.
#[derive(Debug, Default, Clone)]
pub struct EnvironmentMap {
    /// The equirectangular environment texture.
    pub texture: Option<AssetRef<CpuTexture>>,
}

impl EnvironmentMap {
    /// Points the environment at the equirectangular texture asset `texture`.
    pub fn from_asset(texture: AssetUUID) -> Self {
        Self {
            texture: Some(AssetRef::Asset(texture)),
        }
    }
}
