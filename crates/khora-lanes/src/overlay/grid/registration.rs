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

//! The grid overlay family's registration.

use khora_core::agent::{AgentImportance, ExecutionPhase, RegisteredAgent};

use super::{GridFamily, GridLane};

// An overlay draws over what the renderer drew, and is editor and debug viz:
// among the first to give way under budget pressure.
khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: GridFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [crate::render_lane::RENDERER],
        fixed_step: None,
        lanes: [GridLane],
    })
}
