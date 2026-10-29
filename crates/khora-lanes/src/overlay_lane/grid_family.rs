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

//! The grid family: the lane that draws the editor ground grid, and the pass it hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_data::render::GridPassSlot;
use khora_macros::LaneOutputs;

/// The grid family. Its output is presentation: an overlay is what the user
/// sees over the scene, never what the game simulates.
pub struct GridFamily;

impl LaneFamily for GridFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "grid";
    const AGENT: AgentId = AgentId::Grid;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = GridOutputs<'f>;
}

/// What the grid lane writes: its overlay pass.
#[derive(LaneOutputs)]
pub struct GridOutputs<'f> {
    /// The overlay pass, closed into its pass slot.
    #[pass]
    pub pass: Pass<'f, GridPassSlot>,
}
