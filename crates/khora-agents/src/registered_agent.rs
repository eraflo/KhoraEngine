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

//! Every family's agent registration, collected at link time, and the boot
//! that turns them into one [`LaneAgent`] each.
//!
//! A family registers with
//! `inventory::submit! { RegisteredAgent(register_agent! { … }) }`. The
//! newtype exists because `inventory::collect!` cannot name a type of another
//! crate.

use khora_core::agent::registration::{check_registrations_in, AgentRegistration, BootError};
use khora_core::agent::ExecutionPhase;
use khora_core::gpu::DeviceCaps;
use khora_core::Runtime;

use crate::lane_agent::{lane_agent, LaneAgent};

/// One family's registration, as submitted to the collection.
pub struct RegisteredAgent(pub AgentRegistration);

khora_script::inventory::collect!(RegisteredAgent);

/// Every registration submitted to the collection.
pub fn registered_agents() -> Vec<&'static AgentRegistration> {
    khora_script::inventory::iter::<RegisteredAgent>
        .into_iter()
        .map(|registered| &registered.0)
        .collect()
}

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
