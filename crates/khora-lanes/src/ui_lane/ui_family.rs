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

//! The UI family: the lane that draws the game's interface over the frame,
//! and the pass it hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::Pass;
use khora_core::lane::LaneKind;
use khora_data::render::UiPassSlot;
use khora_macros::LaneOutputs;

/// The UI family. Its output is presentation.
pub struct UiFamily;

impl LaneFamily for UiFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "ui";
    const AGENT: AgentId = AgentId::Ui;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = UiOutputs<'f>;
}

/// What the UI lane writes: its pass.
#[derive(LaneOutputs)]
pub struct UiOutputs<'f> {
    /// The UI pass, closed into its pass slot.
    #[pass]
    pub pass: Pass<'f, UiPassSlot>,
}
