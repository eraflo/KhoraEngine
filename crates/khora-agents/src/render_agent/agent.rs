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

//! The render family's registration: the one `LaneAgent` the engine boots
//! for it, its phase and its three lanes.

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::lane::family::LaneFamily;
use khora_lanes::render_lane::{ForwardPlusLane, LitForwardLane, RenderFamily, SimpleUnlitLane};

use crate::registered_agent::RegisteredAgent;

/// The scene renderer's name.
pub const RENDERER: AgentId = <RenderFamily as LaneFamily>::AGENT;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: RenderFamily,
        // After the shadows, which run in an earlier phase and whose frame
        // the lit lanes read from the deck.
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Critical,
        after: [],
        fixed_step: None,
        // The reference image until the DCC lends an envelope.
        default_level: StrategyId::Balanced,
        lanes: [SimpleUnlitLane, LitForwardLane, ForwardPlusLane],
    })
}
