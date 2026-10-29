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

//! The UI family's registration: the one `LaneAgent` the engine boots for
//! it, its phase and its lane.

use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_lanes::ui_lane::{UiFamily, UiLane};

use crate::registered_agent::RegisteredAgent;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: UiFamily,
        // The UI is drawn over the frame; the pass order puts it after the
        // scene, so no agent needs to have run first.
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Important,
        after: [],
        modes: [],
        fixed_step: None,
        lanes: [UiLane],
    })
}
