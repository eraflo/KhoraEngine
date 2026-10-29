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

//! The three overlay families' registrations, what their lanes declare, the
//! boot that checks them, and the agents built from them.

use std::any::TypeId;
use std::sync::Arc;
use std::time::Duration;

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::{AgentId, NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::registration::{check_registrations, AgentRegistration, BootError};
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
use khora_data::render::{
    GizmoPassSlot, GizmoView, GridPassSlot, GridView, RenderWorld, WireframePassSlot, WireframeView,
};
use khora_data::AssetStore;

use super::fixtures::{
    agent_of, gizmo_registration, grid_registration, overlay_registrations, wireframe_registration,
};

// ─── The registrations ──────────────────────────────────────────────────────

/// **Each overlay is its own family.** A family runs one lane a frame, and
/// the grid, the wireframe and the gizmos all draw every frame they have
/// work: so they are three families — `grid`, `wireframe`, `gizmo` — of
/// three agents, each with one lane of its own, presentation, rendering.
/// Checked together, the three are accepted, and the engine builds three
/// agents from them.
#[test]
fn each_overlay_is_its_own_family() {
    let registrations = overlay_registrations();
    let expected = [
        ("grid", AgentId::Grid, "GridLane"),
        ("wireframe", AgentId::Wireframe, "WireframeLane"),
        ("gizmo", AgentId::Gizmo, "GizmoLane"),
    ];

    for (registration, (name, agent, lane)) in registrations.iter().zip(expected) {
        assert_eq!(registration.family_name, name);
        assert_eq!(registration.agent, agent, "{name}");
        assert_eq!(registration.kind, LaneKind::Render, "{name}");
        assert_eq!(registration.semantics, Semantics::Presentation, "{name}");
        let lanes: Vec<&'static str> = registration
            .lanes
            .iter()
            .map(|make| make().name())
            .collect();
        assert_eq!(lanes.len(), 1, "{name}: one lane: {lanes:?}");
        assert!(lanes[0].ends_with(lane), "{name}: the {lane}: {lanes:?}");
    }
    let families: Vec<TypeId> = registrations.iter().map(|r| r.family).collect();
    assert!(
        families[0] != families[1] && families[1] != families[2] && families[0] != families[2],
        "three families"
    );

    let mut runtime = Runtime::new();
    runtime.resources.insert(AssetStore::new());
    let checked = check_registrations(&registrations, &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the three boot together");
    let agents = match lane_agents(&registrations, &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the overlay families boot: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, [AgentId::Grid, AgentId::Wireframe, AgentId::Gizmo]);
}

/// **The overlay agents are optional and run after the renderer.** Each
/// registration is `OUTPUT`, after `Renderer`, `Optional` — debug and editor
/// viz is what budget pressure drops first — in every mode, every frame; the
/// agent built from it says so to the scheduler as one hard dependency.
#[test]
fn the_overlay_agents_are_optional_and_run_after_the_renderer() {
    for registration in overlay_registrations() {
        let name = registration.family_name;
        assert_eq!(registration.phase, ExecutionPhase::OUTPUT, "{name}");
        assert_eq!(registration.after, [AgentId::Renderer], "{name}");
        assert_eq!(registration.importance, AgentImportance::Optional, "{name}");
        assert!(registration.modes.is_empty(), "{name}: in every mode");
        assert_eq!(registration.fixed_step, None, "{name}: every frame");

        let timing = agent_of(registration).execution_timing();
        assert_eq!(timing.default_phase, ExecutionPhase::OUTPUT, "{name}");
        assert_eq!(timing.allowed_phases, [ExecutionPhase::OUTPUT], "{name}");
        assert_eq!(timing.importance, AgentImportance::Optional, "{name}");
        assert_eq!(
            timing.dependencies.len(),
            1,
            "{name}: {:?}",
            timing.dependencies
        );
        assert_eq!(timing.dependencies[0].target, AgentId::Renderer, "{name}");
        assert!(
            matches!(timing.dependencies[0].kind, DependencyKind::Hard),
            "{name}: a hard dependency: {:?}",
            timing.dependencies[0].kind
        );
    }
}

// ─── What the lanes declare ─────────────────────────────────────────────────

/// Asserts the one lane of `registration` reads exactly `inputs` — as
/// `(source, type)` — locks nothing, and writes the pass slot `slot`.
fn assert_declares(
    registration: &'static AgentRegistration,
    inputs: &[(SourceKind, TypeId, &str)],
    slot: TypeId,
) {
    let name = registration.family_name;
    let declaration = (registration.lanes[0])().declaration();
    let declared: Vec<(SourceKind, TypeId)> = declaration
        .inputs()
        .iter()
        .map(|input| (input.kind, input.ty))
        .collect();
    for (kind, ty, what) in inputs {
        assert!(
            declared.contains(&(*kind, *ty)),
            "{name}: reads {what} ({kind:?}): {:?}",
            declaration.inputs()
        );
    }
    assert_eq!(
        declared.len(),
        inputs.len(),
        "{name}: reads nothing else: {:?}",
        declaration.inputs()
    );
    assert!(
        declaration
            .outputs()
            .iter()
            .any(|output| output.kind == SinkKind::Pass && output.ty == slot),
        "{name}: writes its pass slot: {:?}",
        declaration.outputs()
    );
    assert_eq!(
        declaration.outputs().len(),
        1,
        "{name}: and nothing else: {:?}",
        declaration.outputs()
    );
}

/// **The grid lane reads the grid View, the `RenderWorld` and the frame's
/// target**, and writes the grid pass.
#[test]
fn the_grid_lane_declares_what_it_reads_and_writes() {
    assert_declares(
        grid_registration(),
        &[
            (SourceKind::Bus, TypeId::of::<GridView>(), "the grid View"),
            (
                SourceKind::Bus,
                TypeId::of::<RenderWorld>(),
                "the RenderWorld",
            ),
            (
                SourceKind::Frame,
                TypeId::of::<Target>(),
                "the frame's target",
            ),
        ],
        TypeId::of::<GridPassSlot>(),
    );
}

/// **The wireframe lane reads the wireframe View, the `RenderWorld`, the
/// frame's target and the asset store** holding the meshes on the GPU, and
/// writes the wireframe pass.
#[test]
fn the_wireframe_lane_declares_what_it_reads_and_writes() {
    assert_declares(
        wireframe_registration(),
        &[
            (
                SourceKind::Bus,
                TypeId::of::<WireframeView>(),
                "the wireframe View",
            ),
            (
                SourceKind::Bus,
                TypeId::of::<RenderWorld>(),
                "the RenderWorld",
            ),
            (
                SourceKind::Frame,
                TypeId::of::<Target>(),
                "the frame's target",
            ),
            (
                SourceKind::Resource,
                TypeId::of::<AssetStore>(),
                "the asset store",
            ),
        ],
        TypeId::of::<WireframePassSlot>(),
    );
}

/// **The gizmo lane reads the gizmo View, the `RenderWorld` and the frame's
/// target**, and writes the gizmo pass.
#[test]
fn the_gizmo_lane_declares_what_it_reads_and_writes() {
    assert_declares(
        gizmo_registration(),
        &[
            (SourceKind::Bus, TypeId::of::<GizmoView>(), "the gizmo View"),
            (
                SourceKind::Bus,
                TypeId::of::<RenderWorld>(),
                "the RenderWorld",
            ),
            (
                SourceKind::Frame,
                TypeId::of::<Target>(),
                "the frame's target",
            ),
        ],
        TypeId::of::<GizmoPassSlot>(),
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The grid and gizmo families boot on an empty runtime**: they read only
/// the bus and the frame, which boot does not ask for.
#[test]
fn the_grid_and_gizmo_families_boot_on_an_empty_runtime() {
    for registration in [grid_registration(), gizmo_registration()] {
        assert_eq!(
            check_registrations(&[registration], &Runtime::new(), &DeviceCaps::default()),
            Ok(()),
            "{}",
            registration.family_name
        );
    }
}

/// Boot refuses the wireframe family on a runtime without the asset store,
/// naming the family and the resource: the lane could never find a mesh on
/// the GPU.
#[test]
fn boot_refuses_the_wireframe_family_without_the_asset_store() {
    let checked = check_registrations(
        &[wireframe_registration()],
        &Runtime::new(),
        &DeviceCaps::default(),
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "wireframe");
            assert!(
                resource.contains("AssetStore"),
                "the missing resource is the asset store: {resource}"
            );
        }
        other => panic!("expected the asset store to be missing, got {other:?}"),
    }
}

// ─── The agents ─────────────────────────────────────────────────────────────

/// **Each overlay agent reads the frame context, the device and the pipeline
/// system**, without locking; the wireframe's also reads the asset store,
/// and the grid's and the gizmos' do not — the old overlay agent read it for
/// all three. Each writes its own pass slot and nothing else.
#[test]
fn each_overlay_agent_reads_the_frame_context_the_device_and_the_pipeline_system() {
    let slots = [
        TypeId::of::<GridPassSlot>(),
        TypeId::of::<WireframePassSlot>(),
        TypeId::of::<GizmoPassSlot>(),
    ];
    for (registration, slot) in overlay_registrations().into_iter().zip(slots) {
        let name = registration.family_name;
        let contention = agent_of(registration).contention();
        for (what, ty) in [
            ("the frame context", TypeId::of::<Arc<FrameContext>>()),
            ("the device", TypeId::of::<Arc<dyn GraphicsDevice>>()),
            (
                "the pipeline system",
                TypeId::of::<Arc<dyn PipelineSystem>>(),
            ),
        ] {
            assert!(
                contention.reads.contains(&ty),
                "{name}: {what} is read: {:?}",
                contention.reads
            );
        }
        assert_eq!(
            contention.reads.contains(&TypeId::of::<AssetStore>()),
            name == "wireframe",
            "{name}: the asset store is read only by the wireframe"
        );
        assert!(
            contention.writes.is_empty(),
            "{name}: nothing is locked: {:?}",
            contention.writes
        );
        assert_eq!(contention.deck, vec![slot], "{name}: its own slot only");
    }
}

/// **The overlay agents neither contend nor share a slot**, with each other
/// or with the skybox's: they read shared handles and each writes a slot of
/// its own, so one concurrent wave may hold them all.
#[test]
fn the_overlay_agents_neither_contend_nor_share_a_slot() {
    let all: Vec<_> = overlay_registrations()
        .into_iter()
        .map(|registration| agent_of(registration).contention())
        .chain([crate::skybox_family::fixtures::skybox_agent().contention()])
        .collect();

    for (position, one) in all.iter().enumerate() {
        for other in &all[position + 1..] {
            assert!(
                one.conflicts_with(other).is_empty(),
                "the overlays share handles by reading them"
            );
        }
    }
    let mut slots: Vec<TypeId> = all.iter().flat_map(|c| c.deck.clone()).collect();
    let count = slots.len();
    slots.sort();
    slots.dedup();
    assert_eq!(slots.len(), count, "no slot is claimed twice");
}

/// The overlay agents never reach the `World`: they read Views from the bus,
/// so they may run on the worker pool.
#[test]
fn the_overlay_agents_are_world_free() {
    for registration in overlay_registrations() {
        assert_eq!(
            agent_of(registration).access(),
            AgentAccess::Isolated,
            "{}",
            registration.family_name
        );
    }
}

/// **Each overlay agent offers one strategy, `Balanced`**, and reports it as
/// its own agent.
#[test]
fn each_overlay_agent_offers_one_balanced_strategy() {
    for (registration, id) in
        overlay_registrations()
            .into_iter()
            .zip([AgentId::Grid, AgentId::Wireframe, AgentId::Gizmo])
    {
        let mut agent = agent_of(registration);
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

        assert_eq!(offered, [StrategyId::Balanced], "{id:?}");
        let status = agent.report_status();
        assert_eq!(status.agent_id, id);
        assert_eq!(status.current_strategy, StrategyId::Balanced, "{id:?}");
    }
}
