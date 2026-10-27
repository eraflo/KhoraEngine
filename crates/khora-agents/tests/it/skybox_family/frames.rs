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

//! A frame through the skybox agent before any frame target exists: the lane
//! has nothing to paint into, so the agent commits nothing and says why.

use std::sync::Arc;

use khora_core::agent::Agent;
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_data::render::{RenderWorld, SkyboxPassSlot};

use super::fixtures::{skybox_agent, skybox_runtime};
use crate::shadows_family::fixtures::{run_once, SubmitLog};

/// **Without a frame context the sky is not drawn, and the agent names the
/// target.** Between two frames there is no `FrameContext`, so no target to
/// lend: the lane's `target` input is refused, no encoder is opened, the deck
/// holds no skybox pass, and the status says which input was missing.
#[test]
fn a_frame_without_a_frame_context_commits_no_sky_and_names_the_target() {
    let device = Arc::new(SubmitLog::new());
    let runtime = Arc::new(skybox_runtime(&device));
    let mut agent = skybox_agent();
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    bus.publish(RenderWorld::new());
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &bus, &mut deck);

    assert!(
        !deck.contains::<SkyboxPassSlot>(),
        "no skybox pass without a target"
    );
    assert!(device.opened().is_empty(), "no encoder opened");
    let message = agent.report_status().message;
    assert!(
        message.contains("target"),
        "the status names the missing target: {message:?}"
    );
}
