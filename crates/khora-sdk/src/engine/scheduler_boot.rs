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

//! The agents and the scheduler at boot: one lane agent per family linked
//! into the binary, the check that every agent runs in some phase, and the
//! execution scheduler over the DCC's agents.

use khora_agents::lane_agent::{set_fixed_step, LaneAgent};
use khora_agents::registered_agent::{lane_agents_in, registered_agents};
use khora_control::DccService;
use khora_core::agent::{Agent, ExecutionPhase};
use khora_core::Runtime;
use std::sync::{Arc, Mutex};

/// Refuses the boot when a registered agent runs in no phase of `order`: it
/// would be registered and budgeted, yet never run. The families are checked
/// at registration; this catches an agent an application put in the runtime's
/// registry itself.
pub(super) fn refuse_stranded_agents(dcc: &DccService, order: &[ExecutionPhase]) {
    let registry = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for agent in registry.iter() {
        let agent = agent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let phases = agent.execution_timing().allowed_phases;
        // The background runs off the frame: no phase order holds it.
        let runs =
            |phase: &ExecutionPhase| *phase == ExecutionPhase::BACKGROUND || order.contains(phase);
        if !phases.iter().any(runs) {
            panic!(
                "the engine cannot boot: the agent {:?} runs only in {phases:?}, which the app removed",
                agent.id()
            );
        }
    }
}

/// One `LaneAgent` per family registration linked into the binary, each
/// registered with `dcc` for its registration's modes.
///
/// The registrations are checked together first, against the runtime and the
/// device. They are fixed when the program is built, so a refused set is a
/// build defect no frame could recover from: the engine refuses to boot.
pub(super) fn register_lane_agents(
    dcc: &DccService,
    runtime: &Runtime,
    phase_order: &[ExecutionPhase],
) -> Vec<Arc<Mutex<LaneAgent>>> {
    let registrations = registered_agents();
    // What the installed graphics device can do decides which lanes stay on
    // their family's menu; without a device, nothing optional is assumed.
    let caps = runtime
        .look_up::<Arc<dyn khora_core::renderer::GraphicsDevice>>()
        .map(|device| device.caps())
        .unwrap_or_default();
    let mut agents = match lane_agents_in(&registrations, runtime, &caps, phase_order) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the engine cannot boot its lane agents: {refusal}"),
    };
    // The physics step is the game's setting, not the registration's default
    // and never a budget's: the physics family runs at `physics.step`.
    let physics = runtime
        .resources
        .get::<khora_core::physics::PhysicsSettings>()
        .copied()
        .unwrap_or_default();
    for (registration, agent) in registrations.iter().zip(agents.iter_mut()) {
        if registration.agent
            == <khora_lanes::physics::PhysicsFamily as khora_core::lane::family::LaneFamily>::AGENT
        {
            set_fixed_step(agent, physics.step);
        }
    }
    // An application can reach the registry through the runtime before the
    // families are registered: a family's name it took would leave the
    // family never running.
    let taken = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .all_ids();
    if let Some(registration) = registrations
        .iter()
        .find(|registration| taken.contains(&registration.agent))
    {
        panic!(
            "the engine cannot boot its lane agents: the family `{}` negotiates as {:?}, which another agent already is",
            registration.family_name, registration.agent
        );
    }
    registrations
        .iter()
        .zip(agents)
        .map(|(registration, agent)| {
            let agent = Arc::new(Mutex::new(agent));
            dcc.register_agent(agent.clone() as Arc<Mutex<dyn Agent>>, 1.0);
            dcc.agent_registry()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .set_origin(registration.agent, registration.module);
            agent
        })
        .collect()
}

/// The execution scheduler over `dcc`'s agents, running `phase_order`.
pub(super) fn build_scheduler(
    phase_order: &[ExecutionPhase],
    dcc: &mut DccService,
) -> khora_control::ExecutionScheduler {
    // Every registered agent gets a budget: the built-in ones and the ones the
    // application registered before them.
    let registry = dcc.agent_registry().clone();
    let mut scheduler = khora_control::ExecutionScheduler::from_registry(registry);

    // Connect the read-only observation tunnel: the scheduler publishes
    // per-agent cost samples + per-component access snapshots the DCC's
    // cost model and layout advisor consume.
    scheduler.set_telemetry_sender(dcc.event_sender());

    // Run phases in concurrent waves: agents whose contentions are disjoint
    // run at once within a phase. Pass submission stays deterministic
    // (fixed scene→ui→overlay fold).
    scheduler.set_parallel_execution(true);

    // The app's own phases, in the order the lane agents were checked against.
    scheduler.set_phase_order(phase_order);

    let budget_channel = scheduler.budget_channel().clone();
    dcc.connect_budget_channel(budget_channel);
    scheduler
}
