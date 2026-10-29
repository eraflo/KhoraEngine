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

//! The overlay families' registrations: one `LaneAgent` each for the grid,
//! the wireframe and the gizmos, their phase and their lane.

use khora_core::agent::gorna::AgentId;
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_lanes::overlay_lane::{
    GizmoFamily, GizmoLane, GridFamily, GridLane, WireframeFamily, WireframeLane,
};

use crate::registered_agent::RegisteredAgent;

// Overlays draw over what the renderer drew, and are editor and debug viz:
// the first to give way under budget pressure. Their passes composite in the
// order their slots are registered — grid, wireframe, gizmos.

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: GridFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [AgentId::Renderer],
        modes: [],
        fixed_step: None,
        lanes: [GridLane],
    })
}

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: WireframeFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [AgentId::Renderer],
        modes: [],
        fixed_step: None,
        lanes: [WireframeLane],
    })
}

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: GizmoFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [AgentId::Renderer],
        modes: [],
        fixed_step: None,
        lanes: [GizmoLane],
    })
}
