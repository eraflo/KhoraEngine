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

use khora_agents::{
    audio_agent::AudioAgent, overlay_agent::OverlayAgent, physics_agent::PhysicsAgent,
    render_agent::RenderAgent, shadow_agent::ShadowAgent, skybox_agent::SkyboxAgent,
    ui_agent::UiAgent,
};
use khora_core::agent::{Agent, AgentAccess};
use khora_core::physics::ContactBatch;
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_data::flow::{AudioPlaybackWriteback, PhysicsStepResult};
use khora_data::ui::UiImageAtlas;
use std::any::TypeId;
use std::sync::Arc;

/// **The gain the contract was written for.** Both are `SharedWorld`, both sit
/// in `OUTPUT`, and the old "at most one `SharedWorld` per wave" rule kept them
/// apart — not because they contend, but because nothing could say whether they
/// did. Now something can, and the answer is that they do not.
#[test]
fn the_render_and_ui_agents_do_not_contend() {
    let render = RenderAgent::default().contention();
    let ui = UiAgent::default().contention();

    assert!(
        render.conflicts_with(&ui).is_empty(),
        "render and UI may share a concurrent wave"
    );
}

/// Reading is free. Five agents look at the same GPU device and the same
/// per-agent status map every frame; if a shared *read* counted as contention,
/// each of them would land in a wave of its own.
#[test]
fn the_agents_sharing_read_only_handles_do_not_contend() {
    let all = [
        RenderAgent::default().contention(),
        ShadowAgent::default().contention(),
        SkyboxAgent::default().contention(),
        OverlayAgent::default().contention(),
        UiAgent::default().contention(),
    ];

    for (position, one) in all.iter().enumerate() {
        for other in &all[position + 1..] {
            assert!(
                one.conflicts_with(other).is_empty(),
                "the render-family agents share handles by reading them"
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
        RenderAgent::default().contention(),
        SkyboxAgent::default().contention(),
        OverlayAgent::default().contention(),
        UiAgent::default().contention(),
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
// slot by slot, against what each lane actually writes.

/// `ShadowLane` writes the frame's `ShadowFrame` (`shadow_lane/mod.rs`).
#[test]
fn shadow_agent_declares_its_deck_writes() {
    let deck = ShadowAgent::default().contention().deck;

    assert!(
        deck.contains(&TypeId::of::<ShadowFrame>()),
        "the shadow agent's lane writes ShadowFrame; its contention declares {deck:?}"
    );
}

/// `SpatialMixingLane` writes `AudioPlaybackWriteback`
/// (`audio_lane/spatial_mixing.rs`).
#[test]
fn audio_agent_declares_its_deck_writes() {
    let deck = AudioAgent::default().contention().deck;

    assert!(
        deck.contains(&TypeId::of::<AudioPlaybackWriteback>()),
        "the audio agent's lane writes AudioPlaybackWriteback; its contention declares {deck:?}"
    );
}

/// `StandardPhysicsLane` writes both `PhysicsStepResult` and `ContactBatch`
/// (`physics_lane/standard.rs`).
#[test]
fn physics_agent_declares_its_deck_writes() {
    let deck = PhysicsAgent::default().contention().deck;

    for (slot, name) in [
        (TypeId::of::<PhysicsStepResult>(), "PhysicsStepResult"),
        (TypeId::of::<ContactBatch>(), "ContactBatch"),
    ] {
        assert!(
            deck.contains(&slot),
            "the physics agent's lane writes {name}; its contention declares {deck:?}"
        );
    }
}

/// None of the three reaches the `World`: shadow and audio read Views from
/// the bus, and the physics lane only steps the provider (the ECS sync is the
/// `PhysicsFlow`'s and the writeback a `DataSystem`'s). An agent that declares
/// more world access than it uses runs alone in its wave for nothing.
#[test]
fn world_free_agents_are_isolated() {
    let agents: [(&str, AgentAccess); 3] = [
        ("shadow", ShadowAgent::default().access()),
        ("audio", AudioAgent::default().access()),
        ("physics", PhysicsAgent::default().access()),
    ];

    let not_isolated: Vec<_> = agents
        .iter()
        .filter(|(_, access)| *access != AgentAccess::Isolated)
        .collect();
    assert!(
        not_isolated.is_empty(),
        "world-free agents must declare AgentAccess::Isolated; these do not: {not_isolated:?}"
    );
}

/// The UI lane uploads glyphs and images into the shared atlas: that is a
/// lock, and a lock declared as a read lets another agent read a half-written
/// atlas in the same wave.
#[test]
fn ui_agent_locks_the_atlas() {
    let contention = UiAgent::default().contention();
    let atlas = TypeId::of::<Arc<UiImageAtlas>>();

    assert!(
        contention.writes.contains(&atlas),
        "the UI agent mutates the atlas, so it must declare it under `writes`"
    );
    assert!(
        !contention.reads.contains(&atlas),
        "the atlas is locked, not merely read: it must not also sit under `reads`"
    );
}

/// The active camera reaches the renderer through the `CameraView` the SDK
/// prepares the frame from, so the render agent reads the bus and nothing of
/// the `World`.
#[test]
fn render_agent_is_world_free() {
    assert_eq!(
        RenderAgent::default().access(),
        AgentAccess::Isolated,
        "the render agent no longer reads the World"
    );
}
