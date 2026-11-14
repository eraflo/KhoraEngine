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

//! `UnlitInputs`: what the unlit lane reads each frame.

use khora_core::lane::io::SceneTarget;
use khora_core::lane::ClearColor;
use khora_data::render::RenderWorld;
use khora_data::AssetStore;
use khora_macros::LaneInputs;

/// What the unlit lane reads each frame.
#[derive(LaneInputs)]
pub struct UnlitInputs<'f> {
    /// The frame's meshes and views.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour and depth attachments; a target without depth is
    /// refused.
    #[frame]
    pub target: SceneTarget,
    /// The colour the target is cleared to.
    #[frame]
    pub clear_color: ClearColor,
    /// The store holding the meshes on the GPU.
    #[resource]
    pub assets: &'f AssetStore,
}
