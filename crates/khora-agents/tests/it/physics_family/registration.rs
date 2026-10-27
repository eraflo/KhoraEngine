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

//! The physics family's registration, its boot, and the agent built from it:
//! what it declares, the step it runs at, and that no budget changes it.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::{
    AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
};
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, EngineMode, ExecutionPhase, ExecutionTiming,
};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::level::Impact;
use khora_core::lane::LaneKind;
use khora_core::physics::{ContactBatch, PhysicsProvider, PhysicsSettings};
use khora_core::Runtime;
use khora_data::flow::PhysicsStepResult;

use super::fixtures::{physics_agent, physics_registration, physics_runtime, ScriptedProvider};

fn unconstrained() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

fn budget(strategy_id: StrategyId) -> ResourceBudget {
    ResourceBudget {
        strategy_id,
        time_limit: Duration::from_millis(2),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The physics family registers by submitting**, once, as the agent it
/// replaces ran: a `Physics` agent in `TRANSFORM`, critical, in every mode,
/// at a fixed step — the default `physics.step` — with one lane.
#[test]
fn the_physics_family_is_registered_once() {
    let registration = physics_registration();

    assert_eq!(registration.family_name, "physics");
    assert_eq!(registration.agent, AgentId::Physics);
    assert_eq!(registration.kind, LaneKind::Physics);
    assert_eq!(registration.semantics, Semantics::Exact);
    assert_eq!(registration.phase, ExecutionPhase::TRANSFORM);
    assert_eq!(registration.importance, AgentImportance::Critical);
    assert!(
        registration.modes.is_empty(),
        "physics runs in every mode; a paused clock is what stills it"
    );
    assert_eq!(
        registration.fixed_step,
        Some(PhysicsSettings::default().step),
        "the registration runs at the default `physics.step`"
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("StandardPhysicsLane"),
        "the standard physics lane: {lanes:?}"
    );
}

/// **An exact family offers only levels that keep the result.** Physics has
/// one level, of impact `None`: no budget may buy a cheaper simulation that
/// lands bodies elsewhere.
#[test]
fn the_physics_family_offers_one_level_that_keeps_the_result() {
    let lanes: Vec<_> = physics_registration()
        .lanes
        .iter()
        .map(|make| make())
        .collect();
    let levels: Vec<_> = lanes.iter().flat_map(|lane| lane.levels().iter()).collect();

    assert_eq!(levels.len(), 1, "one level: {levels:?}");
    assert_eq!(
        levels[0].impact,
        Impact::None,
        "the level keeps the result: {levels:?}"
    );
}

/// The lane locks the provider for its step — it mutates the simulation, so
/// a read would let another agent see it half-stepped — and its family
/// declares both slots it writes.
#[test]
fn the_physics_lane_locks_the_provider_and_writes_both_slots() {
    let lane = (physics_registration().lanes[0])();
    let declaration = lane.declaration();

    assert!(
        declaration
            .inputs()
            .iter()
            .any(|input| input.kind == SourceKind::Locked
                && input.ty == TypeId::of::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()),
        "the provider, locked: {:?}",
        declaration.inputs()
    );
    for (slot, name) in [
        (TypeId::of::<PhysicsStepResult>(), "PhysicsStepResult"),
        (TypeId::of::<ContactBatch>(), "ContactBatch"),
    ] {
        assert!(
            declaration
                .outputs()
                .iter()
                .any(|output| output.kind == SinkKind::Deck && output.ty == slot),
            "the {name} slot: {:?}",
            declaration.outputs()
        );
    }
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The physics family boots** on a runtime holding a provider: the check
/// accepts it — its one level keeps the result, as an exact family must — and
/// one `Physics` agent is built from it.
#[test]
fn the_physics_family_boots_with_a_provider() {
    let (provider, _steps) = ScriptedProvider::new(Vec::new(), 0);
    let runtime = physics_runtime(&provider.shared());
    let registration = physics_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the physics family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the physics family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::Physics]);
}

/// Boot refuses the physics family on a runtime without a provider, naming
/// the family and the backend: its lane could never step.
#[test]
fn boot_refuses_the_physics_family_without_a_provider() {
    let mut runtime = Runtime::new();
    runtime.resources.insert(PhysicsSettings::default());

    let checked = check_registrations(&[physics_registration()], &runtime, &DeviceCaps::default());

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "physics");
            assert!(
                resource.contains("PhysicsProvider"),
                "the missing entry is the provider: {resource}"
            );
        }
        other => panic!("expected the provider to be missing, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// The physics agent's contention is its lane's: both slots, and the
/// provider's lock — so no other agent touching the provider shares its wave.
#[test]
fn the_physics_agent_writes_both_slots_and_locks_the_provider() {
    let contention = physics_agent().contention();
    let provider = TypeId::of::<Arc<Mutex<Box<dyn PhysicsProvider>>>>();

    for (slot, name) in [
        (TypeId::of::<PhysicsStepResult>(), "PhysicsStepResult"),
        (TypeId::of::<ContactBatch>(), "ContactBatch"),
    ] {
        assert!(
            contention.deck.contains(&slot),
            "{name} is declared: {:?}",
            contention.deck
        );
    }
    assert!(
        contention.writes.contains(&provider),
        "the provider is locked: {:?}",
        contention.writes
    );
}

/// The physics agent never reaches the `World`: the provider sync is a
/// DataSystem's and the write-back another's, so it may run on the worker
/// pool.
#[test]
fn the_physics_agent_is_world_free() {
    assert_eq!(physics_agent().access(), AgentAccess::Isolated);
}

/// The agent runs at the default `physics.step`, in `TRANSFORM`, critical:
/// the scheduler cuts the gameplay time into steps of that size.
#[test]
fn the_physics_agent_runs_at_the_default_physics_step() {
    let timing = physics_agent().execution_timing();

    assert_eq!(timing.fixed_timestep, Some(PhysicsSettings::default().step));
    assert_eq!(timing.allowed_phases, vec![ExecutionPhase::TRANSFORM]);
    assert_eq!(timing.default_phase, ExecutionPhase::TRANSFORM);
    assert_eq!(timing.importance, AgentImportance::Critical);
}

/// **No budget changes the physics step.** The former agent turned GORNA's
/// strategies into steps of 1/30, 1/60 and 1/120 s; the step is a game
/// setting now. Whatever GORNA picks — a level the agent offers, or one it
/// does not — the step stays.
#[test]
fn no_gorna_level_changes_the_physics_step() {
    let mut agent = physics_agent();
    let step = agent.execution_timing().fixed_timestep;
    assert!(step.is_some(), "physics runs at a fixed step");

    let offered: Vec<StrategyId> = agent
        .negotiate(unconstrained())
        .strategies
        .iter()
        .map(|option| option.id)
        .collect();
    let named = offered.iter().copied().chain([
        StrategyId::LowPower,
        StrategyId::Balanced,
        StrategyId::HighPerformance,
        StrategyId::Custom(7),
    ]);
    for strategy in named {
        agent.apply_budget(budget(strategy));
        assert_eq!(
            agent.execution_timing().fixed_timestep,
            step,
            "{strategy:?} left the step alone"
        );
    }
}
