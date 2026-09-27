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

use khora_agents::audio_agent::AudioAgent;
use khora_agents::overlay_agent::OverlayAgent;
use khora_agents::physics_agent::PhysicsAgent;
use khora_agents::render_agent::RenderAgent;
use khora_agents::script_agent::ScriptAgent;
use khora_agents::shadow_agent::ShadowAgent;
use khora_agents::skybox_agent::SkyboxAgent;
use khora_agents::ui_agent::UiAgent;
use khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_core::control::gorna::{AgentId, NegotiationRequest, ResourceConstraints, StrategyId};
use std::time::Duration;

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

#[test]
fn shadow_agent_offers_three_tiers_from_standard_down() {
    assert_eq!(
        offer(ShadowAgent::default()),
        (
            AgentId::ShadowRenderer,
            vec![
                (StrategyId::HighPerformance, (64 + 24) * MIB),
                (StrategyId::Balanced, (16 + 6) * MIB),
                (StrategyId::LowPower, 4 * MIB + 3 * 512 * 1024),
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

#[test]
fn skybox_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(SkyboxAgent::default()),
        (AgentId::Skybox, vec![(StrategyId::Balanced, 0)])
    );
}

#[test]
fn ui_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(UiAgent::default()),
        (AgentId::Ui, vec![(StrategyId::Balanced, MIB)])
    );
}

#[test]
fn physics_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(PhysicsAgent::default()),
        (
            AgentId::Physics,
            vec![
                (StrategyId::LowPower, 0),
                (StrategyId::Balanced, 0),
                (StrategyId::HighPerformance, 0),
            ]
        )
    );
}

#[test]
fn audio_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(AudioAgent::default()),
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
