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

//! The wireframe family: the lane that draws the scene's meshes as edges, and the pass it hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_data::render::WireframePassSlot;
use khora_macros::LaneOutputs;

/// The wireframe family. Its output is presentation: an overlay is what the user
/// sees over the scene, never what the game simulates.
pub struct WireframeFamily;

impl LaneFamily for WireframeFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "wireframe";
    const AGENT: AgentId = AgentId::Wireframe;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = WireframeOutputs<'f>;
}

/// What the wireframe lane writes: its overlay pass.
#[derive(LaneOutputs)]
pub struct WireframeOutputs<'f> {
    /// The overlay pass, closed into its pass slot.
    #[pass]
    pub pass: Pass<'f, WireframePassSlot>,
}
