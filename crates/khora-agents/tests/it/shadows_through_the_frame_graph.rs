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

//! The shadow pass is submitted by the engine, in the frame graph's order,
//! like every other pass: the shadows family's agent commits it into its pass
//! slot and never hands a command buffer to the device itself.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::agent::Agent;
use khora_core::lane::OutputDeck;
use khora_data::render::ShadowPassSlot;

use crate::shadows_family::fixtures::{
    bus_with_a_sun, run_once, shadow_agent, shadows_runtime, SubmitLog,
};

/// **Shadows go through the frame graph.** Run over a frame, the shadow agent
/// submits nothing to the device; every command buffer it finished is in its
/// `ShadowPassSlot`, which it declares, for the engine to submit first, in the
/// registered pass order.
#[test]
fn shadows_go_through_the_frame_graph() {
    let mut agent = shadow_agent();
    let contention = agent.contention();
    let device = Arc::new(SubmitLog::new());
    let runtime = Arc::new(shadows_runtime(&device));
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);

    assert!(
        device.submitted().is_empty(),
        "the shadow agent submitted {:?} itself; the engine submits passes",
        device.submitted()
    );
    let in_slot = deck
        .get::<ShadowPassSlot>()
        .and_then(|slot| slot.0.as_ref())
        .map(|pass| pass.command_buffer);
    assert!(in_slot.is_some(), "the frame's shadow pass is in the slot");
    let finished = device.finished();
    assert!(!finished.is_empty(), "the shadow pass was recorded");
    for finished in finished {
        assert_eq!(
            in_slot,
            Some(finished),
            "a shadow command buffer was finished but is not in the ShadowPassSlot"
        );
    }
    assert!(
        contention.deck.contains(&TypeId::of::<ShadowPassSlot>()),
        "the shadow agent declares the ShadowPassSlot it writes; it declares {:?}",
        contention.deck
    );
}
