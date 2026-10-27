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

//! The skybox family's registration, what its lane declares, the boot that
//! checks it, and the agent built from it.

use std::any::TypeId;
use std::sync::Arc;
use std::time::Duration;

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::{AgentId, NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, DependencyKind, EngineMode, ExecutionPhase,
    ExecutionTiming,
};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::Target;
use khora_core::lane::LaneKind;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::{RenderWorld, SkyboxPassSlot};
use khora_data::IblBaker;

use super::fixtures::{skybox_agent, skybox_registration};

// ─── The registration ───────────────────────────────────────────────────────

/// **The skybox family registers by submitting**, once: a `Skybox` agent,
/// presentation, important — the sky is not debug viz, so it survives budget
/// pressure the optional overlays do not — in every mode, every frame, with
/// one lane.
#[test]
fn the_skybox_family_is_registered_once() {
    let registration = skybox_registration();

    assert_eq!(registration.family_name, "skybox");
    assert_eq!(registration.agent, AgentId::Skybox);
    assert_eq!(registration.kind, LaneKind::Render);
    assert_eq!(registration.semantics, Semantics::Presentation);
    assert_eq!(registration.importance, AgentImportance::Important);
    assert!(registration.modes.is_empty(), "the sky draws in every mode");
    assert_eq!(registration.fixed_step, None, "the sky draws every frame");
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("SkyboxLane"),
        "the skybox lane: {lanes:?}"
    );
}

/// **The skybox registration runs after the renderer**, in `OUTPUT`: the
/// background pass loads the colour and depth the scene pass drew, so the
/// renderer must have recorded first. The agent built from it says so to the
/// scheduler as one hard dependency.
#[test]
fn the_skybox_registration_runs_after_the_renderer() {
    let registration = skybox_registration();
    assert_eq!(registration.phase, ExecutionPhase::OUTPUT);
    assert_eq!(registration.after, [AgentId::Renderer]);

    let timing = skybox_agent().execution_timing();
    assert_eq!(timing.default_phase, ExecutionPhase::OUTPUT);
    assert_eq!(timing.allowed_phases, [ExecutionPhase::OUTPUT]);
    assert_eq!(timing.importance, AgentImportance::Important);
    assert_eq!(timing.dependencies.len(), 1, "{:?}", timing.dependencies);
    assert_eq!(timing.dependencies[0].target, AgentId::Renderer);
    assert!(
        matches!(timing.dependencies[0].kind, DependencyKind::Hard),
        "a hard dependency: {:?}",
        timing.dependencies[0].kind
    );
}

/// The lane declares what it reads — the `RenderWorld` from the bus, the
/// frame's `Target`, the IBL bake from the runtime — and locks nothing; its
/// family declares the pass slot it writes.
#[test]
fn the_skybox_lane_declares_what_it_reads_and_writes() {
    let lane = (skybox_registration().lanes[0])();
    let declaration = lane.declaration();

    let reads = |kind: SourceKind, ty: TypeId| {
        declaration
            .inputs()
            .iter()
            .any(|input| input.kind == kind && input.ty == ty)
    };
    assert!(
        reads(SourceKind::Bus, TypeId::of::<RenderWorld>()),
        "the RenderWorld from the bus: {:?}",
        declaration.inputs()
    );
    assert!(
        reads(SourceKind::Frame, TypeId::of::<Target>()),
        "the frame's target: {:?}",
        declaration.inputs()
    );
    assert!(
        reads(SourceKind::Resource, TypeId::of::<IblBaker>()),
        "the IBL bake: {:?}",
        declaration.inputs()
    );
    assert!(
        declaration
            .inputs()
            .iter()
            .all(|input| input.kind != SourceKind::Locked),
        "nothing is locked: {:?}",
        declaration.inputs()
    );
    assert!(
        declaration
            .outputs()
            .iter()
            .any(|output| output.kind == SinkKind::Pass
                && output.ty == TypeId::of::<SkyboxPassSlot>()),
        "the skybox pass: {:?}",
        declaration.outputs()
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The skybox family boots** on a runtime holding the IBL bake: the check
/// accepts it, and one `Skybox` agent is built from it. The `FrameContext` it
/// reads its target from is not there at boot — the driver inserts it once a
/// frame begins — and boot does not ask for it.
#[test]
fn the_skybox_family_boots_with_the_ibl_baker_and_no_frame_context() {
    let mut runtime = Runtime::new();
    runtime.resources.insert(IblBaker::new());
    let registration = skybox_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the skybox family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the skybox family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::Skybox]);
}

/// Boot refuses the skybox family on a runtime without the IBL bake, naming
/// the family and the resource: the lane could never sample an environment.
#[test]
fn boot_refuses_the_skybox_family_without_the_ibl_baker() {
    let checked = check_registrations(
        &[skybox_registration()],
        &Runtime::new(),
        &DeviceCaps::default(),
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "skybox");
            assert!(
                resource.contains("IblBaker"),
                "the missing resource is the IBL bake: {resource}"
            );
        }
        other => panic!("expected the IBL bake to be missing, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// **The skybox agent reads the frame context**, where its lane's target is
/// published, and, without locking, the device and pipeline system it
/// prepares its lane with and the IBL bake its lane samples; it writes its
/// pass slot and nothing else. An undeclared read would be refused by
/// `EngineContext::resource`.
#[test]
fn the_skybox_agent_reads_the_frame_context_the_device_and_the_ibl_baker() {
    let contention = skybox_agent().contention();

    for (name, ty) in [
        ("the frame context", TypeId::of::<Arc<FrameContext>>()),
        ("the device", TypeId::of::<Arc<dyn GraphicsDevice>>()),
        (
            "the pipeline system",
            TypeId::of::<Arc<dyn PipelineSystem>>(),
        ),
        ("the IBL bake", TypeId::of::<IblBaker>()),
    ] {
        assert!(
            contention.reads.contains(&ty),
            "{name} is read: {:?}",
            contention.reads
        );
    }
    assert!(
        contention.writes.is_empty(),
        "nothing is locked: {:?}",
        contention.writes
    );
    assert_eq!(
        contention.deck,
        vec![TypeId::of::<SkyboxPassSlot>()],
        "the skybox pass slot, and only it"
    );
}

/// The skybox agent never reaches the `World`: it reads Views from the bus,
/// so it may run on the worker pool.
#[test]
fn the_skybox_agent_is_world_free() {
    assert_eq!(skybox_agent().access(), AgentAccess::Isolated);
}

/// **The skybox agent offers one strategy, `Balanced`**, and reports it as
/// the `Skybox` agent: there is one fixed background pass, nothing to trade.
#[test]
fn the_skybox_agent_offers_one_balanced_strategy() {
    let mut agent = skybox_agent();

    let offered: Vec<StrategyId> = agent
        .negotiate(NegotiationRequest {
            target_latency: Duration::from_micros(16_667),
            priority_weight: 1.0,
            constraints: ResourceConstraints::default(),
            current_mode: EngineMode::Playing,
            agent_timing: ExecutionTiming::default(),
        })
        .strategies
        .iter()
        .map(|option| option.id)
        .collect();

    assert_eq!(offered, [StrategyId::Balanced]);
    let status = agent.report_status();
    assert_eq!(status.agent_id, AgentId::Skybox);
    assert_eq!(status.current_strategy, StrategyId::Balanced);
}
