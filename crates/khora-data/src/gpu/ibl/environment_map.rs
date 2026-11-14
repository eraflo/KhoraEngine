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

/// Selects the scene's environment source for the IBL bake.
///
/// Registered as a shared resource by the host application. When it names an
/// equirectangular texture asset (an HDR `.hdr`/`.exr` keeps the dynamic range
/// that makes reflections read well), the bake projects it onto the environment
/// cube; absent — or naming an asset that has not been loaded — the procedural
/// sky is baked instead, so a scene without an authored environment still
/// lights correctly.
///
/// The selection is read every tick: selecting another texture, or the
/// selected texture changing in the store, bakes again.
#[derive(Debug, Default, Clone)]
pub struct EnvironmentMap {
    /// Equirectangular environment texture, as a loaded `CpuTexture` asset.
    pub texture: Option<AssetUUID>,
}

impl EnvironmentMap {
    /// Points the environment at an equirectangular texture asset.
    pub fn from_asset(texture: AssetUUID) -> Self {
        Self {
            texture: Some(texture),
        }
    }
}
