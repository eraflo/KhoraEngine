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

//! The shadows family: the lanes that fill the shadow atlases, and what every
//! one of them hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_data::render::ShadowPassSlot;
use khora_macros::LaneOutputs;

/// The shadows family. Its output is presentation: a cheaper level fills a
/// smaller atlas, never a different set of shadowed lights.
pub struct ShadowsFamily;

impl LaneFamily for ShadowsFamily {
    const KIND: LaneKind = LaneKind::Shadow;
    const NAME: &'static str = "shadows";
    const AGENT: AgentId = AgentId::ShadowRenderer;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ShadowsOutputs<'f>;
}

/// What every shadows lane writes: the depth passes filling the atlases, which
/// the engine submits first, and the frame's shadows, which the lit lanes read.
#[derive(LaneOutputs)]
pub struct ShadowsOutputs<'f> {
    /// The depth passes, closed into the shadow pass slot.
    #[pass]
    pub pass: Pass<'f, ShadowPassSlot>,
    /// The atlases to bind and each shadowed light's entry.
    #[deck]
    pub frame: ShadowFrame,
}
