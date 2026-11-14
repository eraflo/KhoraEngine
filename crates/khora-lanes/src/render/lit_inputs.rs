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

//! `LitInputs`: what the lit lanes read each frame.

use khora_core::lane::io::SceneTarget;
use khora_core::lane::ClearColor;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_data::render::RenderWorld;
use khora_data::{AssetStore, IblBaker};
use khora_macros::LaneInputs;

/// What the lit lanes read each frame.
#[derive(LaneInputs)]
pub struct LitInputs<'f> {
    /// The frame's meshes, lights and views.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The frame's colour and depth attachments; a target without depth is
    /// refused.
    #[frame]
    pub target: SceneTarget,
    /// The colour the target is cleared to.
    #[frame]
    pub clear_color: ClearColor,
    /// The shadow atlases and entries, when the shadows family drew them.
    #[deck]
    pub shadows: Option<&'f ShadowFrame>,
    /// The store holding the meshes on the GPU.
    #[resource]
    pub assets: &'f AssetStore,
    /// The image-based lighting bake.
    #[resource]
    pub ibl: &'f IblBaker,
}
