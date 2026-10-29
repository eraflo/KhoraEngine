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

//! The UI family's registration, what its lane declares, the boot that
//! checks it, and the agent built from it.

use std::any::TypeId;
use std::sync::Arc;
use std::time::Duration;

use khora_agents::registered_agent::lane_agents;
use khora_agents::render_agent::RenderAgent;
use khora_core::agent::gorna::{AgentId, NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, EngineMode, ExecutionPhase, ExecutionTiming,
};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::Target;
use khora_core::lane::LaneKind;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::UiPassSlot;
use khora_data::ui::{UiImageAtlas, UiScene};
use khora_data::AssetStore;

use super::fixtures::{ui_agent, ui_registration, NoText};
use crate::overlay_families::fixtures::{agent_of, overlay_registrations};

// ─── The registration ───────────────────────────────────────────────────────

/// **The UI family registers by submitting**, once: the `Ui` agent, of kind
/// `Ui` — the old lane said `Render` — presentation, in every mode, every
/// frame, with one lane, the `UiLane` (the old one was `"UiRender"`).
#[test]
fn the_ui_family_is_registered_once() {
    let registration = ui_registration();

    assert_eq!(registration.family_name, "ui");
    assert_eq!(registration.agent, AgentId::Ui);
    assert_eq!(registration.kind, LaneKind::Ui);
    assert_eq!(registration.semantics, Semantics::Presentation);
    assert!(registration.modes.is_empty(), "the UI draws in every mode");
    assert_eq!(registration.fixed_step, None, "the UI draws every frame");
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(lanes[0].ends_with("UiLane"), "the UI lane: {lanes:?}");
}

/// **The UI registration runs in OUTPUT**, important — the interface is not
/// debug viz, so it survives the budget pressure the overlays give way to —
/// and after no agent: its pass loads the colour, and the pass order, not
/// the agents' order, puts it after the scene. The agent built from it says
/// so to the scheduler.
#[test]
fn the_ui_registration_runs_in_output() {
    let registration = ui_registration();
    assert_eq!(registration.phase, ExecutionPhase::OUTPUT);
    assert_eq!(registration.importance, AgentImportance::Important);
    assert!(
        registration.after.is_empty(),
        "after no agent: {:?}",
        registration.after
    );

    let timing = ui_agent().execution_timing();
    assert_eq!(timing.default_phase, ExecutionPhase::OUTPUT);
    assert_eq!(timing.allowed_phases, [ExecutionPhase::OUTPUT]);
    assert_eq!(timing.importance, AgentImportance::Important);
    assert!(
        timing.dependencies.is_empty(),
        "no dependency: {:?}",
        timing.dependencies
    );
}

// ─── What the lane declares ─────────────────────────────────────────────────

/// **The UI lane declares what it reads and writes**: the `UiScene` from the
/// bus, the frame's `Target`, and from the runtime the image atlas and the
/// text renderer — nothing else, nothing locked; its family writes the UI
/// pass slot and nothing else.
#[test]
fn the_ui_lane_declares_what_it_reads_and_writes() {
    let declaration = (ui_registration().lanes[0])().declaration();
    let declared: Vec<(SourceKind, TypeId)> = declaration
        .inputs()
        .iter()
        .map(|input| (input.kind, input.ty))
        .collect();

    for (kind, ty, what) in [
        (SourceKind::Bus, TypeId::of::<UiScene>(), "the UiScene"),
        (
            SourceKind::Frame,
            TypeId::of::<Target>(),
            "the frame's target",
        ),
        (
            SourceKind::Resource,
            TypeId::of::<Arc<UiImageAtlas>>(),
            "the image atlas",
        ),
        (
            SourceKind::Resource,
            TypeId::of::<Arc<dyn TextRenderer>>(),
            "the text renderer",
        ),
    ] {
        assert!(
            declared.contains(&(kind, ty)),
            "reads {what} ({kind:?}): {:?}",
            declaration.inputs()
        );
    }
    assert_eq!(
        declared.len(),
        4,
        "nothing else: {:?}",
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
    assert_eq!(
        declaration
            .outputs()
            .iter()
            .map(|output| (output.kind, output.ty))
            .collect::<Vec<_>>(),
        [(SinkKind::Pass, TypeId::of::<UiPassSlot>())],
        "the UI pass, and only it"
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// A runtime holding what the UI reads at boot: the image atlas and a text
/// renderer.
fn ui_boot_runtime() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::new(UiImageAtlas::new()));
    runtime
        .backends
        .insert(Arc::new(NoText) as Arc<dyn TextRenderer>);
    runtime
}

/// **The UI family boots** on a runtime holding the image atlas and a text
/// renderer: the check accepts it, and one `Ui` agent is built from it. The
/// `FrameContext` it reads its target from is not there at boot, and boot
/// does not ask for it.
#[test]
fn the_ui_family_boots_with_the_atlas_and_a_text_renderer() {
    let runtime = ui_boot_runtime();
    let registration = ui_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the UI family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the UI family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::Ui]);
}

/// Asserts the check refuses the UI family on `runtime`, naming the family
/// and a resource whose name holds `missing`.
fn assert_boot_refused_for_want_of(runtime: &Runtime, missing: &str) {
    match check_registrations(&[ui_registration()], runtime, &DeviceCaps::default()) {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "ui");
            assert!(
                resource.contains(missing),
                "the missing resource is the {missing}: {resource}"
            );
        }
        other => panic!("expected the {missing} to be missing, got {other:?}"),
    }
}

/// Boot refuses the UI family on a runtime without the image atlas: the lane
/// could never sample a UI image.
#[test]
fn boot_refuses_the_ui_family_without_the_image_atlas() {
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::new(NoText) as Arc<dyn TextRenderer>);

    assert_boot_refused_for_want_of(&runtime, "UiImageAtlas");
}

/// Boot refuses the UI family on a runtime without a text renderer: the lane
/// could never draw a text.
#[test]
fn boot_refuses_the_ui_family_without_a_text_renderer() {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::new(UiImageAtlas::new()));

    assert_boot_refused_for_want_of(&runtime, "TextRenderer");
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// **The UI reads the image atlas without locking it.** The `image_on_gpu`
/// DataSystem places every UI image before any agent runs; the lane only
/// reads the atlas's view and rects. The old agent locked it, which kept
/// every other reader of the atlas out of its wave.
#[test]
fn the_ui_reads_the_image_atlas_without_locking_it() {
    let contention = ui_agent().contention();
    let atlas = TypeId::of::<Arc<UiImageAtlas>>();

    assert!(
        contention.reads.contains(&atlas),
        "the atlas is read: {:?}",
        contention.reads
    );
    assert!(
        !contention.writes.contains(&atlas),
        "and not locked: {:?}",
        contention.writes
    );
}

/// **The UI agent reads the frame context, the atlas, the text renderer, the
/// device and the pipeline system**, locks nothing, and writes its pass slot
/// and nothing else. An undeclared read would be refused by
/// `EngineContext::resource`.
#[test]
fn the_ui_agent_reads_the_frame_context_the_atlas_the_text_renderer_the_device_and_the_pipelines() {
    let contention = ui_agent().contention();

    for (name, ty) in [
        ("the frame context", TypeId::of::<Arc<FrameContext>>()),
        ("the image atlas", TypeId::of::<Arc<UiImageAtlas>>()),
        ("the text renderer", TypeId::of::<Arc<dyn TextRenderer>>()),
        ("the device", TypeId::of::<Arc<dyn GraphicsDevice>>()),
        (
            "the pipeline system",
            TypeId::of::<Arc<dyn PipelineSystem>>(),
        ),
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
        vec![TypeId::of::<UiPassSlot>()],
        "the UI pass slot, and only it"
    );
}

/// UI images reach the atlas through the `image_on_gpu` residency, before
/// the agents run; the UI never reads the decoded textures, so it declares
/// no read of the `AssetStore` (a needless read would serialise its wave).
#[test]
fn the_ui_agent_leaves_the_asset_store_to_the_residency() {
    assert!(
        !ui_agent()
            .contention()
            .may_reach(TypeId::of::<AssetStore>()),
        "the UI reads no texture from the AssetStore"
    );
}

/// **The UI agent contends with no render-family agent**: the renderer, the
/// shadows, the skybox and the three overlays read shared handles and each
/// writes a slot of its own, so one concurrent wave may hold them all.
#[test]
fn the_ui_agent_contends_with_no_render_family_agent() {
    let ui = ui_agent().contention();
    let others = [
        ("renderer", RenderAgent::default().contention()),
        (
            "shadows",
            crate::shadows_family::fixtures::shadow_agent().contention(),
        ),
        (
            "skybox",
            crate::skybox_family::fixtures::skybox_agent().contention(),
        ),
    ]
    .into_iter()
    .chain(overlay_registrations().into_iter().map(|registration| {
        (
            registration.family_name,
            agent_of(registration).contention(),
        )
    }));

    for (name, other) in others {
        assert!(
            ui.conflicts_with(&other).is_empty(),
            "the UI and the {name} may share a wave: {:?}",
            ui.conflicts_with(&other)
        );
    }
}

/// The UI agent never reaches the `World`: it reads its scene from the bus,
/// so it may run on the worker pool.
#[test]
fn the_ui_agent_is_world_free() {
    assert_eq!(ui_agent().access(), AgentAccess::Isolated);
}

/// **The UI agent offers one strategy, `Balanced`**, the one its old agent
/// offered, and reports it as the `Ui` agent.
#[test]
fn the_ui_agent_offers_one_balanced_strategy() {
    let mut agent = ui_agent();

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
    assert_eq!(status.agent_id, AgentId::Ui);
    assert_eq!(status.current_strategy, StrategyId::Balanced);
}
