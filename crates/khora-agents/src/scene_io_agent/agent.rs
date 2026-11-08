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

//! The `scene_io` family's registration: `ExecutionPhase::BACKGROUND`,
//! `AgentImportance::Optional`, the scene load lane at its one level.

use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_lanes::scene_io_lane::{SceneIoFamily, SceneIoLane};

use crate::registered_agent::RegisteredAgent;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: SceneIoFamily,
        phase: ExecutionPhase::BACKGROUND,
        importance: AgentImportance::Optional,
        after: [],
        fixed_step: None,
        lanes: [SceneIoLane],
    })
}
