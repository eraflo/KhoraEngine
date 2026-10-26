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

//! Tests of the execution scheduler, one file per concern.

mod agent_ids;
mod concurrent_exec;
mod deck_writes;
mod engine_mode;
mod fixed_agent_sub_clocks;
mod panicking_agents;
mod physics_step_is_precision;
mod physics_time_stays_on_gameplay_time;
mod published_fixed_steps;
mod sim_steps;
mod waves;
