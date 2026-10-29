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

//! The gizmo family: the lane that draws the editor and debug line gizmos, and the pass it hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_data::render::GizmoPassSlot;
use khora_macros::LaneOutputs;

/// The gizmo family. Its output is presentation: an overlay is what the user
/// sees over the scene, never what the game simulates.
pub struct GizmoFamily;

impl LaneFamily for GizmoFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "gizmo";
    const AGENT: AgentId = AgentId::Gizmo;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = GizmoOutputs<'f>;
}

/// What the gizmo lane writes: its overlay pass.
#[derive(LaneOutputs)]
pub struct GizmoOutputs<'f> {
    /// The overlay pass, closed into its pass slot.
    #[pass]
    pub pass: Pass<'f, GizmoPassSlot>,
}
