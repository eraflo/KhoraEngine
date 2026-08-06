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
    overlay_agent::OverlayAgent, render_agent::RenderAgent, shadow_agent::ShadowAgent,
    skybox_agent::SkyboxAgent, ui_agent::UiAgent,
};
use khora_core::agent::Agent;

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
