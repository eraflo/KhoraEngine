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

//! The script family's registration, its boot, and the agent built from it:
//! what it declares, where it runs, and the level it starts on.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::Budget;
use khora_core::lane::level::Impact;
use khora_core::script::{BehaviorWriteback, CommandBuffer};
use khora_core::Runtime;
use khora_data::flow::{PrefabCatalogView, ReservationView, ScriptView};
use khora_lanes::script_lane::ScriptRuntime;

use super::fixtures::{runtime_with, script_agent, script_registration};
use crate::protocol::request;

// ─── The registration ───────────────────────────────────────────────────────

/// **The script family registers by submitting**, once, as the agent it
/// replaces ran: a `Script` agent named by the core's `SCRIPT`, in
/// `TRANSFORM`, critical, every frame, an exact family of one lane, starting
/// on its whole level.
#[test]
fn the_script_family_is_registered_once() {
    let registration = script_registration();

    assert_eq!(registration.family_name, "script");
    assert_eq!(registration.agent, khora_core::script::SCRIPT);
    assert_eq!(registration.agent, AgentId::new("script"));
    assert_eq!(registration.semantics, Semantics::Exact);
    assert_eq!(registration.phase, ExecutionPhase::TRANSFORM);
    assert_eq!(registration.importance, AgentImportance::Critical);
    assert_eq!(registration.fixed_step, None, "it runs every frame");
    assert_eq!(
        registration.default_level,
        Some(StrategyId::Balanced),
        "it starts on its whole level"
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("BudgetedScriptLane"),
        "the budgeted script lane: {lanes:?}"
    );
}

/// **An exact family offers only levels that keep the result.** Two: the
/// critical level, then the whole one, both of impact `None` — a cut defers
/// a turn and never changes what a turn computes.
#[test]
fn the_script_family_offers_a_critical_level_then_a_whole_one() {
    let lanes: Vec<_> = script_registration()
        .lanes
        .iter()
        .map(|make| make())
        .collect();
    let levels: Vec<(StrategyId, &str, Impact)> = lanes
        .iter()
        .flat_map(|lane| lane.levels().iter())
        .map(|level| (level.id, level.name, level.impact))
        .collect();

    assert_eq!(
        levels,
        [
            (StrategyId::LowPower, "critical", Impact::None),
            (StrategyId::Balanced, "whole", Impact::None),
        ]
    );
}

/// The lane reads the three Views the script flows publish, locks the
/// scripting world for its run, takes a budget, and its family declares both
/// slots it writes.
#[test]
fn the_script_lane_reads_its_views_locks_the_scripting_world_and_writes_both_slots() {
    let lane = (script_registration().lanes[0])();
    let declaration = lane.declaration();
    let reads = |kind: SourceKind, ty: TypeId| {
        declaration
            .inputs()
            .iter()
            .any(|input| input.kind == kind && input.ty == ty)
    };

    for (view, name) in [
        (TypeId::of::<ScriptView>(), "ScriptView"),
        (TypeId::of::<ReservationView>(), "ReservationView"),
        (TypeId::of::<PrefabCatalogView>(), "PrefabCatalogView"),
    ] {
        assert!(
            reads(SourceKind::Bus, view),
            "the {name} View, from the bus: {:?}",
            declaration.inputs()
        );
    }
    assert!(
        reads(
            SourceKind::Locked,
            TypeId::of::<Arc<Mutex<ScriptRuntime>>>()
        ),
        "the scripting world, locked: {:?}",
        declaration.inputs()
    );
    assert!(
        reads(SourceKind::Budget, TypeId::of::<Budget>()),
        "the budget: {:?}",
        declaration.inputs()
    );
    for (slot, name) in [
        (TypeId::of::<CommandBuffer>(), "CommandBuffer"),
        (TypeId::of::<BehaviorWriteback>(), "BehaviorWriteback"),
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

/// **The script family boots** on a runtime holding the scripting world: the
/// check accepts it, and one `script` agent is built from it.
#[test]
fn the_script_family_boots_with_a_scripting_world() {
    let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
    let runtime = runtime_with(&scripts);
    let registration = script_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the script family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the script family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![khora_core::script::SCRIPT]);
}

/// Boot refuses the script family on a runtime without the scripting world,
/// naming the family and the entry: its lane could never run a behaviour.
#[test]
fn boot_refuses_the_script_family_without_a_scripting_world() {
    let checked = check_registrations(
        &[script_registration()],
        &Runtime::new(),
        &DeviceCaps::default(),
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "script");
            assert!(
                resource.contains("ScriptRuntime"),
                "the missing entry is the scripting world: {resource}"
            );
        }
        other => panic!("expected the scripting world to be missing, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// The script agent's contention is its lane's: both slots, and the
/// scripting world's lock — so no other agent touching that world shares
/// its wave.
#[test]
fn the_script_agent_writes_both_slots_and_locks_the_scripting_world() {
    let contention = script_agent().contention();

    for (slot, name) in [
        (TypeId::of::<CommandBuffer>(), "CommandBuffer"),
        (TypeId::of::<BehaviorWriteback>(), "BehaviorWriteback"),
    ] {
        assert!(
            contention.deck.contains(&slot),
            "{name} is declared: {:?}",
            contention.deck
        );
    }
    assert!(
        contention.may_lock(TypeId::of::<Arc<Mutex<ScriptRuntime>>>()),
        "the scripting world is locked: {:?}",
        contention.writes
    );
}

/// The agent runs every frame in `TRANSFORM`, critical: gameplay decides what
/// the frame is about.
#[test]
fn the_script_agent_runs_every_frame_in_transform_as_critical() {
    let agent = script_agent();
    let timing = agent.execution_timing();

    let step = agent
        .place()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .fixed_step();
    assert_eq!(step, None, "its place holds no fixed step");
    assert_eq!(timing.allowed_phases, vec![ExecutionPhase::TRANSFORM]);
    assert_eq!(timing.default_phase, ExecutionPhase::TRANSFORM);
    assert_eq!(timing.importance, AgentImportance::Critical);
}

/// **The script agent starts on its whole level**: before any frame, the row
/// it would run is `Balanced`.
#[test]
fn the_script_agent_starts_on_its_whole_level() {
    assert_eq!(script_agent().report().row, StrategyId::Balanced);
}

/// **An offer answers with nothing measured.** Before any frame, with no
/// scripting world to price from, the agent offers both its levels,
/// cheapest first, holding no graphics memory.
#[test]
fn the_script_agent_offers_before_any_frame() {
    let mut agent = script_agent();
    let offer = agent.offer(&request());

    let offered: Vec<(StrategyId, u64)> = offer
        .rows
        .iter()
        .map(|row| (row.id, row.cost.memory.gpu))
        .collect();
    assert_eq!(
        offered,
        [(StrategyId::LowPower, 0), (StrategyId::Balanced, 0)]
    );
}
