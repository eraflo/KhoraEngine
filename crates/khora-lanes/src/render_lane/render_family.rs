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

//! The render family: the lanes that draw the scene, and the two passes every
//! one of them hands on — the opaque draws, and the blended ones the engine
//! folds the sky in front of.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::{Pass, SceneTarget};
use khora_core::lane::ClearColor;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_data::render::{RenderWorld, ScenePassSlot, TransparentPassSlot};
use khora_data::{AssetStore, IblBaker};
use khora_macros::{LaneInputs, LaneOutputs};

/// The render family. Its output is presentation: a cheaper level draws the
/// same scene with less light, never a different set of objects.
pub struct RenderFamily;

impl LaneFamily for RenderFamily {
    const NAME: &'static str = "render";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = RenderOutputs<'f>;
}

/// What every render lane writes: the opaque scene, then the blended draws.
#[derive(LaneOutputs)]
pub struct RenderOutputs<'f> {
    /// The opaque draws, clearing the frame's colour and depth.
    #[pass]
    pub scene: Pass<'f, ScenePassSlot>,
    /// The blended draws, composited over the opaque scene and the sky.
    #[pass]
    pub transparent: Pass<'f, TransparentPassSlot>,
}

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
