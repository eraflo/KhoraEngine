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

//! The shadows family's registration, what its lane declares, the boot that
//! checks it, and the agent built from it.

use std::any::TypeId;
use std::sync::Arc;

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{Agent, AgentAccess, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::LaneKind;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::flow::ShadowView;
use khora_data::render::{RenderWorld, ShadowPassSlot};
use khora_data::AssetStore;

use super::fixtures::{shadow_agent, shadows_registration, shadows_runtime, SubmitLog};

// ─── The registration ───────────────────────────────────────────────────────

/// **The shadows family registers by submitting**, once, as the agent it
/// replaces ran: a `ShadowRenderer` agent in `OBSERVE` — before the lit lanes
/// of `OUTPUT` read its frame — important, in every mode, every frame, with
/// one lane.
#[test]
fn the_shadows_family_is_registered_once() {
    let registration = shadows_registration();

    assert_eq!(registration.family_name, "shadows");
    assert_eq!(registration.agent, AgentId::ShadowRenderer);
    assert_eq!(registration.kind, LaneKind::Shadow);
    assert_eq!(registration.semantics, Semantics::Presentation);
    assert_eq!(registration.phase, ExecutionPhase::OBSERVE);
    assert_eq!(registration.importance, AgentImportance::Important);
    assert!(registration.modes.is_empty(), "shadows draw in every mode");
    assert_eq!(registration.fixed_step, None, "shadows draw every frame");
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane, three levels: {lanes:?}");
    assert!(
        lanes[0].ends_with("ShadowsLane"),
        "the shadows lane: {lanes:?}"
    );
}

/// The lane declares what it reads — the `RenderWorld` and the `ShadowView`
/// from the bus, the asset store from the runtime — and locks nothing; its
/// family declares the pass slot and the deck slot it writes.
#[test]
fn the_shadows_lane_declares_what_it_reads_and_writes() {
    let lane = (shadows_registration().lanes[0])();
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
        reads(SourceKind::Bus, TypeId::of::<ShadowView>()),
        "the ShadowView from the bus: {:?}",
        declaration.inputs()
    );
    assert!(
        reads(SourceKind::Resource, TypeId::of::<AssetStore>()),
        "the asset store, for the meshes on the GPU: {:?}",
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
    let writes = |kind: SinkKind, ty: TypeId| {
        declaration
            .outputs()
            .iter()
            .any(|output| output.kind == kind && output.ty == ty)
    };
    assert!(
        writes(SinkKind::Pass, TypeId::of::<ShadowPassSlot>()),
        "the shadow pass: {:?}",
        declaration.outputs()
    );
    assert!(
        writes(SinkKind::Deck, TypeId::of::<ShadowFrame>()),
        "the shadow frame: {:?}",
        declaration.outputs()
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The shadows family boots** on a runtime holding the asset store: the
/// check accepts it, and one `ShadowRenderer` agent is built from it.
#[test]
fn the_shadows_family_boots_with_the_asset_store() {
    let runtime = shadows_runtime(&Arc::new(SubmitLog::new()));
    let registration = shadows_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the shadows family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the shadows family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::ShadowRenderer]);
}

/// Boot refuses the shadows family on a runtime without the asset store,
/// naming the family and the resource: the lane could never draw a mesh.
#[test]
fn boot_refuses_the_shadows_family_without_the_asset_store() {
    let checked = check_registrations(
        &[shadows_registration()],
        &Runtime::new(),
        &DeviceCaps::default(),
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "shadows");
            assert!(
                resource.contains("AssetStore"),
                "the missing resource is the asset store: {resource}"
            );
        }
        other => panic!("expected the asset store to be missing, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// **The shadow agent declares both its slots**: the `ShadowFrame` the lit
/// lanes read and the `ShadowPassSlot` the engine submits. A slot written but
/// not declared would lose one writer's value at the merge of a concurrent
/// wave.
#[test]
fn the_shadow_agent_declares_both_its_slots() {
    let deck = shadow_agent().contention().deck;

    assert!(
        deck.contains(&TypeId::of::<ShadowFrame>()),
        "the shadow frame is declared: {deck:?}"
    );
    assert!(
        deck.contains(&TypeId::of::<ShadowPassSlot>()),
        "the shadow pass slot is declared: {deck:?}"
    );
}

/// The shadow agent reads, without locking, the device and the pipeline
/// system it prepares its lane with, and the asset store its lane reads: an
/// undeclared one would be refused by `EngineContext::resource`.
#[test]
fn the_shadow_agent_reads_the_device_the_pipelines_and_the_asset_store() {
    let contention = shadow_agent().contention();

    for (name, ty) in [
        ("the device", TypeId::of::<Arc<dyn GraphicsDevice>>()),
        (
            "the pipeline system",
            TypeId::of::<Arc<dyn PipelineSystem>>(),
        ),
        ("the asset store", TypeId::of::<AssetStore>()),
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
}

/// The shadow agent never reaches the `World`: it reads Views from the bus,
/// so it may run on the worker pool.
#[test]
fn the_shadow_agent_is_world_free() {
    assert_eq!(shadow_agent().access(), AgentAccess::Isolated);
}
