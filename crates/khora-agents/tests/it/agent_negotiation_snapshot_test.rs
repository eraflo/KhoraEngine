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

//! What every agent offers GORNA, pinned: the strategies, in order, with their
//! VRAM quote, for an unconstrained request.
//!
//! Reorganising where lanes live must not change what the arbiter is offered.
//! The render agent derives its offer from its lanes' `strategy_name()`, so a
//! lane that goes missing or is renamed drops a strategy here. Timings are left
//! out: they come from cost estimators, which are free to evolve.
//!
//! Nothing here needs a GPU: `negotiate` runs on the DCC thread against a stub
//! scene.

use khora_agents::lane_agent::lane_agent;
use khora_agents::overlay_agent::OverlayAgent;
use khora_agents::render_agent::RenderAgent;
use khora_agents::script_agent::ScriptAgent;
use khora_agents::ui_agent::UiAgent;
use khora_core::agent::gorna::{AgentId, NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_core::gpu::DeviceCaps;
use std::time::Duration;

use crate::audio_family::audio_registration;
use crate::physics_family::fixtures::physics_agent;
use crate::shadows_family::fixtures::{shadow_agent, LEVEL_MEMORY};
use crate::skybox_family::fixtures::skybox_agent;

const MIB: u64 = 1024 * 1024;

fn unconstrained() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

/// The agent's id and its offer as `(strategy, estimated_vram)`, in the order
/// the agent listed them.
fn offer(mut agent: impl Agent) -> (AgentId, Vec<(StrategyId, u64)>) {
    let response = agent.negotiate(unconstrained());
    let strategies = response
        .strategies
        .iter()
        .map(|s| (s.id, s.estimated_vram))
        .collect();
    (agent.id(), strategies)
}

#[test]
fn render_agent_offers_one_strategy_per_lane_in_registration_order() {
    assert_eq!(
        offer(RenderAgent::default()),
        (
            AgentId::Renderer,
            vec![
                (StrategyId::LowPower, 0),
                (StrategyId::Balanced, 4096),
                (StrategyId::Custom(1), 4096),
                (StrategyId::HighPerformance, 4096 + 8 * MIB),
            ]
        )
    );
}

/// The shadows family's agent, before any frame: one row per level of its
/// one lane, cheapest first, each quoting the graphics memory of its atlases —
/// 5.5, 22 and 88 MiB.
#[test]
fn shadow_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(shadow_agent()),
        (
            AgentId::ShadowRenderer,
            vec![
                (StrategyId::LowPower, LEVEL_MEMORY[0]),
                (StrategyId::Balanced, LEVEL_MEMORY[1]),
                (StrategyId::HighPerformance, LEVEL_MEMORY[2]),
            ]
        )
    );
}

#[test]
fn overlay_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(OverlayAgent::default()),
        (AgentId::Overlay, vec![(StrategyId::Balanced, MIB)])
    );
}

/// The skybox family's agent, before any frame: one `Balanced` row, quoting
/// the graphics memory of its one level — the 80-byte uniform block.
#[test]
fn skybox_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(skybox_agent()),
        (AgentId::Skybox, vec![(StrategyId::Balanced, 80)])
    );
}

#[test]
fn ui_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(UiAgent::default()),
        (AgentId::Ui, vec![(StrategyId::Balanced, MIB)])
    );
}

/// The physics family's agent: one level, which keeps the result, and no
/// VRAM. Its step is a game setting, so GORNA has no step to choose.
#[test]
fn physics_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(physics_agent()),
        (AgentId::Physics, vec![(StrategyId::Balanced, 0)])
    );
}

/// The audio family's agent, before any frame: one row per voice level, no
/// VRAM.
#[test]
fn audio_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(lane_agent(audio_registration(), &DeviceCaps::default())),
        (
            AgentId::Audio,
            vec![
                (StrategyId::LowPower, 0),
                (StrategyId::Balanced, 0),
                (StrategyId::HighPerformance, 0),
            ]
        )
    );
}

#[test]
fn script_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(ScriptAgent::default()),
        (
            AgentId::Script,
            vec![
                (StrategyId::LowPower, 0),
                (StrategyId::Balanced, 0),
                (StrategyId::HighPerformance, 0),
            ]
        )
    );
}
