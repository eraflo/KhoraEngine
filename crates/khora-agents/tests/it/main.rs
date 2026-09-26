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

//! khora-agents integration tests, compiled as one test binary.
//!
//! Each file under `tests/` used to be its own executable, and each executable
//! is a full link of wgpu, rapier and egui. One binary links once.

mod asset_loading_test;
mod collision_to_behavior_test;
mod contention_test;
mod forward_plus_test;
mod frame_e2e_test;
mod garbage_collector_test;
mod input_from_script_test;
mod physics_agent_tests;
mod physics_lane_contacts_test;
mod render_agent_gorna_test;
mod script_delivery_test;
mod shipped_script_runs_test;
