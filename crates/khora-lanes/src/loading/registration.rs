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

//! The `loading` family's registration: `ExecutionPhase::BACKGROUND`,
//! `AgentImportance::Optional`, the rush lane then the paced lane.

use super::{LoadingFamily, PacedLane, RushLane};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{AgentImportance, ExecutionPhase};

use khora_core::agent::RegisteredAgent;

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: LoadingFamily,
        phase: ExecutionPhase::BACKGROUND,
        importance: AgentImportance::Optional,
        after: [],
        fixed_step: None,
        // Everything filed is loaded until the DCC lends an envelope.
        default_level: StrategyId::HighPerformance,
        lanes: [RushLane, PacedLane],
    })
}
