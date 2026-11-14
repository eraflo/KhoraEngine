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

//! The render family's registration, what its lanes declare, the boot that
//! checks it, and the agent built from it.

use std::any::TypeId;
use std::sync::Arc;

use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::protocol::Offer;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::{check_registrations, BootError};
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::AnyLane;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::{PassSlot, Target};
use khora_core::lane::level::Impact;
use khora_core::lane::ClearColor;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::{RenderWorld, ResourceId, ScenePassSlot, TransparentPassSlot};
use khora_data::{AssetStore, IblBaker};

use super::fixtures::{
    headless_render_runtime, offer_request, render_agent, render_registration, EVERY_CAP,
};
use crate::contention_test::{placed, wave_conflicts, Placed};
use crate::families::overlay::fixtures::{agent_of, overlay_registrations};
use crate::families::shadows::fixtures::{shadow_agent, shadows_registration};
use crate::families::skybox::fixtures::skybox_agent;
use crate::families::ui::fixtures::ui_agent;

const MIB: u64 = 1024 * 1024;

/// The lane of the render family whose type name ends with `name`.
fn lane_named(name: &str) -> Box<dyn AnyLane> {
    render_registration()
        .lanes
        .iter()
        .map(|make| make())
        .find(|lane| lane.name().ends_with(name))
        .unwrap_or_else(|| panic!("the render family has a `{name}`"))
}

/// `ids`, sorted by name.
fn by_name(mut ids: Vec<StrategyId>) -> Vec<StrategyId> {
    ids.sort_by_key(|id| format!("{id:?}"));
    ids
}

/// The rows `offer` names, sorted by name.
fn row_ids(offer: &Offer) -> Vec<StrategyId> {
    by_name(offer.rows.iter().map(|row| row.id).collect())
}

/// The rows the render agent offers on a device of `caps`, before any frame,
/// sorted.
fn menu_on(caps: &DeviceCaps) -> Vec<StrategyId> {
    row_ids(&render_agent(caps).offer(&offer_request(1)))
}

/// `ids`, sorted.
fn sorted<T: Ord>(mut ids: Vec<T>) -> Vec<T> {
    ids.sort();
    ids
}

/// The two pass slots every render lane writes, sorted.
fn scene_slots() -> Vec<TypeId> {
    sorted(vec![
        TypeId::of::<ScenePassSlot>(),
        TypeId::of::<TransparentPassSlot>(),
    ])
}

/// Whether `lane` declares a read of `ty` from `kind`.
fn reads(lane: &dyn AnyLane, kind: SourceKind, ty: TypeId) -> bool {
    lane.declaration()
        .inputs()
        .iter()
        .any(|input| input.kind == kind && input.ty == ty)
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The render family registers by submitting**, once: the `renderer`
/// agent — the name the skybox and the overlays wait on — presentation,
/// critical, in every mode, every frame, with three lanes: the unlit one,
/// the forward one and Forward+. The PBR lane is gone: it drew the image the
/// forward lane draws.
#[test]
fn the_render_family_is_registered_once_with_three_lanes() {
    let registration = render_registration();

    assert_eq!(registration.family_name, "render");
    assert_eq!(registration.agent, AgentId::new("renderer"));
    assert_eq!(registration.agent, khora_lanes::render::RENDERER);
    assert_eq!(registration.semantics, Semantics::Presentation);
    assert_eq!(registration.importance, AgentImportance::Critical);
    assert_eq!(registration.fixed_step, None, "the scene draws every frame");
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name().rsplit("::").next().unwrap_or_default())
        .collect();
    assert_eq!(
        sorted(lanes),
        ["ForwardPlusLane", "LitForwardLane", "SimpleUnlitLane"],
        "three lanes, no PBR lane"
    );
}

/// **The render family runs in `OUTPUT`, after the shadows by phase.** The
/// lit lanes read the shadow frame the shadows family writes in `OBSERVE`,
/// an earlier phase: the order comes from the phases and the deck read boot
/// checks, not from a dependency on the shadows agent's name. The agent built
/// from it is critical, runs only in `OUTPUT`, and waits on nobody.
#[test]
fn the_render_family_runs_in_output_after_the_shadows_by_phase() {
    let registration = render_registration();
    assert_eq!(registration.phase, ExecutionPhase::OUTPUT);
    assert!(
        registration.after.is_empty(),
        "no dependency by name: {:?}",
        registration.after
    );
    let order = ExecutionPhase::DEFAULT_ORDER;
    let place = |phase| order.iter().position(|p| *p == phase);
    assert!(
        place(shadows_registration().phase) < place(registration.phase),
        "the shadows' phase runs before the renderer's"
    );

    let timing = render_agent(&EVERY_CAP).execution_timing();
    assert_eq!(timing.phase, ExecutionPhase::OUTPUT);
    assert_eq!(timing.phase, ExecutionPhase::OUTPUT);
    assert_eq!(timing.importance, AgentImportance::Critical);
    assert!(timing.dependencies.is_empty(), "{:?}", timing.dependencies);
}

/// **The render family starts on `Balanced`, the forward lane**, whatever
/// order the menu lists its lanes in: before any frame the agent runs the
/// default level.
#[test]
fn the_render_family_starts_on_balanced() {
    assert_eq!(
        render_registration().default_level,
        Some(StrategyId::Balanced)
    );
    let offer = render_agent(&EVERY_CAP).offer(&offer_request(1));
    assert_eq!(offer.current, StrategyId::Balanced);
}

/// **One level per lane, and their impacts.** The unlit lane is `LowPower`
/// and a difference a player sees (no light at all); the forward lane is
/// `Balanced` and Forward+ `HighPerformance`, both the reference image — the
/// forward lane only quotes worse on a frame past its light caps.
#[test]
fn each_render_lane_offers_one_level_of_its_impact() {
    for (lane, id, impact) in [
        ("SimpleUnlitLane", StrategyId::LowPower, Impact::Major),
        ("LitForwardLane", StrategyId::Balanced, Impact::None),
        ("ForwardPlusLane", StrategyId::HighPerformance, Impact::None),
    ] {
        let levels: Vec<(StrategyId, Impact)> = lane_named(lane)
            .levels()
            .iter()
            .map(|level| (level.id, level.impact))
            .collect();
        assert_eq!(levels, [(id, impact)], "{lane}");
    }
}

/// **Three rows on a capable device, each at its quality and memory.**
/// Before any frame nothing is priced, so each row is offered at its level's
/// impact and the graphics memory the old render agent quoted for it: none
/// for the unlit lane, 4 KiB for the forward lane, 4 KiB and the 8 MiB of
/// light lists for Forward+.
#[test]
fn the_render_agent_offers_three_rows_at_their_quality_and_memory() {
    let offer = render_agent(&EVERY_CAP).offer(&offer_request(1));

    assert_eq!(offer.agent, AgentId::new("renderer"));
    assert_eq!(
        row_ids(&offer),
        by_name(vec![
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance,
        ]),
        "no `Custom(1)` row: the PBR lane is gone"
    );
    for (id, quality, memory) in [
        (StrategyId::LowPower, 0.4, 0),
        (StrategyId::Balanced, 1.0, 4096),
        (StrategyId::HighPerformance, 1.0, 4096 + 8 * MIB),
    ] {
        let row = offer
            .rows
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("{id:?} is offered"));
        assert_eq!(row.quality, quality, "{id:?}'s quality");
        assert_eq!(row.cost.memory.gpu, memory, "{id:?}'s graphics memory");
    }
}

/// **Forward+ needs storage buffers and compute shaders.** On a device
/// lacking either, or with no device at all, it leaves the menu; the unlit
/// and forward lanes stay.
#[test]
fn forward_plus_leaves_the_menu_without_storage_buffers_or_compute() {
    let without = [
        DeviceCaps::default(),
        DeviceCaps {
            storage_buffers: true,
            compute: false,
        },
        DeviceCaps {
            storage_buffers: false,
            compute: true,
        },
    ];
    for caps in without {
        assert_eq!(
            menu_on(&caps),
            by_name(vec![StrategyId::LowPower, StrategyId::Balanced]),
            "{caps:?}: no Forward+"
        );
    }
    assert!(
        menu_on(&EVERY_CAP).contains(&StrategyId::HighPerformance),
        "a device with both keeps Forward+"
    );
}

// ─── What the lanes declare ─────────────────────────────────────────────────

/// **Every render lane writes the scene and the transparent passes**, and no
/// deck slot: its family's two pass outputs. The agent contends for exactly
/// those two slots.
#[test]
fn every_render_lane_writes_the_scene_and_transparent_passes() {
    for lane in ["SimpleUnlitLane", "LitForwardLane", "ForwardPlusLane"] {
        let declaration = lane_named(lane).declaration();
        let passes: Vec<TypeId> = declaration
            .outputs()
            .iter()
            .map(|output| {
                assert_eq!(output.kind, SinkKind::Pass, "{lane}: {:?}", output.field);
                output.ty
            })
            .collect();
        assert_eq!(
            sorted(passes),
            scene_slots(),
            "{lane}: the two scene passes"
        );
    }

    assert_eq!(
        sorted(render_agent(&EVERY_CAP).contention().deck),
        scene_slots(),
        "the agent contends for the two pass slots"
    );
}

/// **The scene pass reads the shadow atlas, always.** A lit lane binds the
/// shadows family's atlas, or its own neutral one when there is none, so the
/// scene pass is ordered after the shadow pass whatever was drawn; it writes
/// the colour and the depth.
#[test]
fn the_scene_pass_reads_the_shadow_atlas() {
    let descriptor = ScenePassSlot::descriptor();
    assert!(
        descriptor.reads.contains(&ResourceId::ShadowAtlas),
        "{:?}",
        descriptor.reads
    );
    assert!(descriptor.writes.contains(&ResourceId::Color));
    assert!(descriptor.writes.contains(&ResourceId::Depth));
}

/// **The lit lanes read the scene, the frame, the shadow frame, the meshes
/// and the IBL bake**, and lock nothing. The shadow frame is a deck read of
/// an earlier phase; the pipelines come through the pass, never as a runtime
/// resource a headless boot does not hold.
#[test]
fn the_lit_lanes_declare_what_they_read() {
    for name in ["LitForwardLane", "ForwardPlusLane"] {
        let lane = lane_named(name);
        for (kind, ty, what) in [
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
                SourceKind::Frame,
                TypeId::of::<ClearColor>(),
                "the clear colour",
            ),
            (
                SourceKind::Deck,
                TypeId::of::<ShadowFrame>(),
                "the shadow frame",
            ),
            (
                SourceKind::Resource,
                TypeId::of::<AssetStore>(),
                "the asset store",
            ),
            (
                SourceKind::Resource,
                TypeId::of::<IblBaker>(),
                "the IBL bake",
            ),
        ] {
            assert!(
                reads(lane.as_ref(), kind, ty),
                "{name} reads {what}: {:?}",
                lane.declaration().inputs()
            );
        }
        let declaration = lane.declaration();
        assert!(
            declaration
                .inputs()
                .iter()
                .all(|input| input.kind != SourceKind::Locked),
            "{name} locks nothing: {:?}",
            declaration.inputs()
        );
        assert!(
            !reads(
                lane.as_ref(),
                SourceKind::Resource,
                TypeId::of::<Arc<dyn PipelineSystem>>()
            ),
            "{name} takes its pipelines from the pass"
        );
    }
}

/// The unlit lane reads the scene, the frame and the meshes, and no pipeline
/// system resource.
#[test]
fn the_unlit_lane_declares_what_it_reads() {
    let lane = lane_named("SimpleUnlitLane");
    for (kind, ty, what) in [
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
    ] {
        assert!(
            reads(lane.as_ref(), kind, ty),
            "the unlit lane reads {what}: {:?}",
            lane.declaration().inputs()
        );
    }
    assert!(
        !reads(
            lane.as_ref(),
            SourceKind::Resource,
            TypeId::of::<Arc<dyn PipelineSystem>>()
        ),
        "the unlit lane takes its pipelines from the pass"
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The render family boots after the shadows family**, on a runtime
/// holding the asset store and the IBL bake: the lit lanes' read of the
/// shadow frame is written by an earlier phase. One agent each.
#[test]
fn the_render_family_boots_after_the_shadows_family() {
    let runtime = headless_render_runtime();
    let registrations = [shadows_registration(), render_registration()];

    assert_eq!(
        check_registrations(&registrations, &runtime, &EVERY_CAP),
        Ok(()),
        "the check accepts the render family after the shadows"
    );
    let agents = match lane_agents(&registrations, &runtime, &EVERY_CAP) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the render family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::new("shadows"), AgentId::new("renderer")]);
}

/// **Boot checks the shadow frame read.** Without the shadows family no
/// earlier phase writes the shadow frame, and boot says so, naming the slot:
/// the `#[deck] Option<&ShadowFrame>` input is what orders the renderer after
/// the shadows.
#[test]
fn boot_refuses_the_render_family_without_a_shadow_frame_writer() {
    let checked = check_registrations(
        &[render_registration()],
        &headless_render_runtime(),
        &EVERY_CAP,
    );

    match checked {
        Err(BootError::DeckReadWithoutEarlierWriter { family, slot, .. }) => {
            assert_eq!(family, "render");
            assert!(slot.ends_with("ShadowFrame"), "the slot named: {slot}");
        }
        other => panic!("expected the shadow frame read refused, got {other:?}"),
    }
}

/// **A headless boot accepts the render family.** No device and no pipeline
/// system in the runtime — a server, a test — and the check still accepts
/// the family: nothing it declares is the pipeline system.
#[test]
fn a_headless_boot_accepts_the_render_family() {
    let runtime = headless_render_runtime();
    assert!(
        !runtime.holds(TypeId::of::<Arc<dyn PipelineSystem>>()),
        "the runtime holds no pipeline system"
    );
    assert!(!runtime.holds(TypeId::of::<Arc<dyn GraphicsDevice>>()));

    let registrations = [shadows_registration(), render_registration()];
    assert_eq!(
        check_registrations(&registrations, &runtime, &DeviceCaps::default()),
        Ok(())
    );
}

/// Boot refuses the render family on a runtime without the IBL bake, naming
/// the family and the resource.
#[test]
fn boot_refuses_the_render_family_without_the_ibl_baker() {
    let mut runtime = Runtime::new();
    runtime.resources.insert(AssetStore::new());
    let checked = check_registrations(
        &[shadows_registration(), render_registration()],
        &runtime,
        &EVERY_CAP,
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "render");
            assert!(
                resource.ends_with("IblBaker"),
                "the resource named: {resource}"
            );
        }
        other => panic!("expected the IBL bake refused, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// **The render agent reads what its lanes read, and locks nothing**: the
/// frame context its target comes from, the device and the pipeline system it
/// lends its lanes, the asset store and the IBL bake.
#[test]
fn the_render_agent_reads_and_locks_nothing() {
    let contention = render_agent(&EVERY_CAP).contention();
    for (name, ty) in [
        ("the frame context", TypeId::of::<Arc<FrameContext>>()),
        ("the device", TypeId::of::<Arc<dyn GraphicsDevice>>()),
        (
            "the pipeline system",
            TypeId::of::<Arc<dyn PipelineSystem>>(),
        ),
        ("the asset store", TypeId::of::<AssetStore>()),
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
}

/// **The render agent contends with no other render-side agent.** The sky,
/// the UI and the three overlays read the same shared handles and each
/// writes slots of its own, so one concurrent wave may hold them all. The
/// renderer reads the shadow frame the shadows write in an earlier phase:
/// the two never share a wave, so that read is not counted
/// ([`wave_conflicts`]). No slot is claimed twice.
#[test]
fn the_render_agent_contends_with_no_other_render_side_agent() {
    let render = placed(&render_agent(&EVERY_CAP));
    let others: Vec<(&'static str, Placed)> = [
        ("shadows", placed(&shadow_agent())),
        ("skybox", placed(&skybox_agent())),
        ("ui", placed(&ui_agent())),
    ]
    .into_iter()
    .chain(
        overlay_registrations()
            .into_iter()
            .map(|registration| (registration.family_name, placed(&agent_of(registration)))),
    )
    .collect();

    for (name, other) in &others {
        let conflicts = wave_conflicts(&render, other);
        assert!(
            conflicts.is_empty(),
            "the renderer and the {name} may share a wave: {conflicts:?}"
        );
    }
    let slots: Vec<TypeId> = std::iter::once(&render)
        .chain(others.iter().map(|(_, other)| other))
        .flat_map(|placed| placed.contention.deck.clone())
        .collect();
    let mut unique = slots.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), slots.len(), "no slot is claimed twice");
}
