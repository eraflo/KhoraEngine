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

//! The boot that turns every family's registration into one [`LaneAgent`].
//!
//! The registrations are collected in core
//! ([`khora_core::agent::registration`]), so a family registers wherever its
//! lanes live.

use khora_core::agent::registration::{check_registrations_in, AgentRegistration, BootError};
pub use khora_core::agent::registration::{registered_agents, RegisteredAgent};
use khora_core::agent::ExecutionPhase;
use khora_core::gpu::DeviceCaps;
use khora_core::Runtime;

use crate::agent::{lane_agent, LaneAgent};

/// Checks `registrations` together against `runtime` and `caps`, then builds
/// one [`LaneAgent`] per registration, in order. A refused set builds none.
pub fn lane_agents(
    registrations: &[&'static AgentRegistration],
    runtime: &Runtime,
    caps: &DeviceCaps,
) -> Result<Vec<LaneAgent>, BootError> {
    lane_agents_in(registrations, runtime, caps, ExecutionPhase::DEFAULT_ORDER)
}

/// [`lane_agents`] for a scheduler running `phase_order`, the order the
/// registrations' deck reads are checked against.
pub fn lane_agents_in(
    registrations: &[&'static AgentRegistration],
    runtime: &Runtime,
    caps: &DeviceCaps,
    phase_order: &[ExecutionPhase],
) -> Result<Vec<LaneAgent>, BootError> {
    check_registrations_in(registrations, runtime, caps, phase_order)?;
    Ok(registrations
        .iter()
        .map(|registration| lane_agent(registration, caps))
        .collect())
}
