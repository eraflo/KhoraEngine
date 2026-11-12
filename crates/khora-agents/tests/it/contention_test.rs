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

//! What the shipped agents actually declare.
//!
//! The scheduler's own tests prove the *mechanism* on synthetic declarations.
//! They cannot catch the failure mode that matters here: an agent that declares
//! more than it touches breaks nothing, reddens nothing, and quietly serialises
//! a wave that could have run concurrently. Only the real declarations can show
//! that, so they are asserted here.

use khora_core::agent::{Agent, Contention, ExecutionPhase};
use khora_data::ui::UiImageAtlas;
use std::any::TypeId;
use std::sync::Arc;

use crate::overlay_families::fixtures::{
    agent_of, gizmo_registration, grid_registration, wireframe_registration,
};
use crate::render_family::fixtures::{render_agent, EVERY_CAP};
use crate::ui_family::fixtures::ui_agent;

/// One agent as the scheduler groups it: the phase it runs in and what it
/// declares.
pub(crate) struct Placed {
    pub(crate) phase: ExecutionPhase,
    pub(crate) contention: Contention,
}

/// `agent`'s phase and declaration.
pub(crate) fn placed<A: Agent>(agent: &A) -> Placed {
    Placed {
        phase: agent.execution_timing().default_phase,
        contention: agent.contention(),
    }
}

/// Where `phase` runs in the frame's default phase order.
fn rank(phase: ExecutionPhase) -> usize {
    ExecutionPhase::DEFAULT_ORDER
        .iter()
        .position(|p| *p == phase)
        .unwrap_or_else(|| panic!("{phase:?} is a phase of the frame's order"))
}

/// What `one` and `other` would fight over in a wave, leaving out a deck slot
/// one reads and the other writes in an **earlier** phase. Waves are formed
/// within a phase, and the boot accepts a deck read only of a slot an earlier
/// phase wrote, so such a pair never shares a wave: the reader sees the slot
/// as that phase left it. Anything else — a resource locked by one and read
/// or locked by the other, a slot both write, a slot read and written within
/// one phase — is reported.
pub(crate) fn wave_conflicts(one: &Placed, other: &Placed) -> Vec<TypeId> {
    let read_after_written = |reader: &Placed, writer: &Placed, slot: &TypeId| {
        reader.contention.deck_reads.contains(slot)
            && writer.contention.deck.contains(slot)
            && rank(writer.phase) < rank(reader.phase)
    };
    one.contention
        .conflicts_with(&other.contention)
        .into_iter()
        .filter(|slot| {
            !read_after_written(one, other, slot) && !read_after_written(other, one, slot)
        })
        .collect()
}

/// **The gain the contract was written for.** The render and UI agents both
/// sit in `OUTPUT`, and what they declare is all that decides whether they
/// share a wave: two agents run at once unless their contentions conflict.
/// The render and UI agents' do not, so they may share a concurrent wave.
#[test]
fn the_render_and_ui_agents_do_not_contend() {
    let render = render_agent(&EVERY_CAP).contention();
    let ui = ui_agent().contention();

    assert!(
        render.conflicts_with(&ui).is_empty(),
        "render and UI may share a concurrent wave"
    );
}

/// Reading is free. No two of the seven render-family agents would fight in
/// a wave: what they share at run time, they only read, and a shared *read*
/// is not contention — if it were, each of them would land in a wave of its
/// own. A deck slot one of them reads from another's earlier phase (the
/// renderer reads the shadows' frame) keeps the two in different phases,
/// never in one wave, so it is not counted ([`wave_conflicts`]).
#[test]
fn the_agents_sharing_read_only_handles_do_not_contend() {
    let all = [
        ("renderer", placed(&render_agent(&EVERY_CAP))),
        (
            "shadows",
            placed(&crate::shadows_family::fixtures::shadow_agent()),
        ),
        (
            "skybox",
            placed(&crate::skybox_family::fixtures::skybox_agent()),
        ),
        ("grid", placed(&agent_of(grid_registration()))),
        ("wireframe", placed(&agent_of(wireframe_registration()))),
        ("gizmo", placed(&agent_of(gizmo_registration()))),
        ("ui", placed(&ui_agent())),
    ];

    for (position, (name, one)) in all.iter().enumerate() {
        for (other_name, other) in &all[position + 1..] {
            let conflicts = wave_conflicts(one, other);
            assert!(
                conflicts.is_empty(),
                "the {name} and the {other_name} share handles by reading them: {conflicts:?}"
            );
        }
    }
}

/// Every render-family agent writes its own deck slot — the one thing among
/// them that genuinely could collide, and the reason `deck` is part of the same
/// declaration rather than a method beside it.
#[test]
fn each_render_agent_writes_a_slot_of_its_own() {
    let slots: Vec<_> = [
        render_agent(&EVERY_CAP).contention(),
        crate::shadows_family::fixtures::shadow_agent().contention(),
        crate::skybox_family::fixtures::skybox_agent().contention(),
        agent_of(grid_registration()).contention(),
        agent_of(wireframe_registration()).contention(),
        agent_of(gizmo_registration()).contention(),
        ui_agent().contention(),
    ]
    .iter()
    .flat_map(|c| c.deck.clone())
    .collect();

    let mut unique = slots.clone();
    unique.sort();
    unique.dedup();

    assert_eq!(unique.len(), slots.len(), "no slot is claimed twice");
}

// ─── Every deck slot a lane writes is declared by its agent ─────────────────
//
// A lane writes through the deck its agent lends it. A slot written but not
// declared breaks nothing on a serial frame and silently loses one writer's
// data at the merge of a concurrent wave — so the declaration is pinned here,
// slot by slot, against what each lane actually writes. A family on the typed
// contract declares its slots through its outputs (audio: `audio_family.rs`,
// physics: `physics_family/registration.rs`, shadows:
// `shadows_family/registration.rs`, skybox: `skybox_family/registration.rs`,
// ui: `ui_family/registration.rs`).

/// The `image_on_gpu` DataSystem uploads every UI image into the shared atlas
/// before any agent runs; the UI lane only reads the atlas's view and rects.
/// So the UI agent declares the atlas as a read: a lock would keep every
/// other reader of the atlas out of its wave for nothing.
#[test]
fn ui_agent_reads_the_atlas_without_locking_it() {
    let contention = ui_agent().contention();
    let atlas = TypeId::of::<Arc<UiImageAtlas>>();

    assert!(
        contention.reads.contains(&atlas),
        "the UI agent reads the atlas, so it declares it under `reads`"
    );
    assert!(
        !contention.writes.contains(&atlas),
        "the atlas is read, not locked: it must not sit under `writes`"
    );
}

/// **The script family's agent leaves the engine's channels and the demand to
/// the intake.** The `script_intake` data system drains the reload and event
/// channels and announces to the demand before the flow projects; the
/// family's lane only runs behaviors. Declaring a lock on a channel it no longer
/// drains would serialise every other user of that channel for nothing. It
/// still locks the scripting world its lane runs against.
#[test]
fn the_script_agent_locks_neither_engine_channel_nor_reaches_the_demand() {
    use khora_core::event::Channel;
    use khora_core::projection::Interest;
    use khora_core::script::{ScriptEvent, ScriptReads};
    use khora_lanes::script::ScriptRuntime;
    use khora_script::reload::ScriptReload;
    use std::sync::Mutex;

    let contention = crate::script_family::fixtures::script_agent().contention();

    assert!(
        !contention.may_reach(TypeId::of::<Channel<ScriptReload>>()),
        "the reload channel is the intake's"
    );
    assert!(
        !contention.may_reach(TypeId::of::<Channel<ScriptEvent>>()),
        "the event channel is the intake's"
    );
    assert!(
        !contention.may_reach(TypeId::of::<Arc<Interest<ScriptReads>>>()),
        "the demand is announced to by the intake"
    );
    assert!(
        contention.may_lock(TypeId::of::<Arc<Mutex<ScriptRuntime>>>()),
        "the lane still runs against the scripting world"
    );
}
