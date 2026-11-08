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

//! The `loading` family's agent: chooses between the rush lane and the paced
//! lane, and installs the chosen one's layer at `ExecutionPhase::BACKGROUND`.

mod agent;

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::LaneFamily;
use khora_lanes::loading_lane::LoadingFamily;

/// The `loading` family's agent, by name.
pub const LOADING: AgentId = <LoadingFamily as LaneFamily>::AGENT;
