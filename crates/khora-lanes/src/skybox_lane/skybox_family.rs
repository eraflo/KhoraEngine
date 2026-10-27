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

//! The skybox family: the lane that paints the environment behind the scene,
//! and the pass it hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_data::render::SkyboxPassSlot;
use khora_macros::LaneOutputs;

/// The skybox family. Its output is presentation: the sky is what a player
/// sees behind the scene, never what the game simulates.
pub struct SkyboxFamily;

impl LaneFamily for SkyboxFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "skybox";
    const AGENT: AgentId = AgentId::Skybox;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = SkyboxOutputs<'f>;
}

/// What the skybox lane writes: the background pass, which the engine folds
/// between the scene's opaque and blended passes.
#[derive(LaneOutputs)]
pub struct SkyboxOutputs<'f> {
    /// The background pass, closed into the skybox pass slot.
    #[pass]
    pub pass: Pass<'f, SkyboxPassSlot>,
}
