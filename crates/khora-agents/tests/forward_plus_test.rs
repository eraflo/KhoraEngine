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

//! Tests for the RenderAgent GORNA surface (negotiate / apply_budget /
//! report_status).  Per CLAUDE.md the agent exposes only the `Agent` trait,
//! so these tests drive it strictly through that interface.

use khora_agents::render_agent::RenderAgent;
use khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_core::control::gorna::{
    NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
};
use std::collections::HashMap;
use std::time::Duration;

fn unconstrained_req() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

fn budget(strategy_id: StrategyId) -> ResourceBudget {
    ResourceBudget {
        strategy_id,
        time_limit: Duration::from_millis(16),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}

#[test]
fn test_negotiate_returns_three_strategies_when_unconstrained() {
    let mut agent = RenderAgent::default();
    let res = agent.negotiate(unconstrained_req());
    assert_eq!(res.strategies.len(), 3);
}

#[test]
fn test_negotiate_vram_constrained_returns_only_low_power() {
    let mut agent = RenderAgent::default();
    let req = NegotiationRequest {
        constraints: ResourceConstraints {
            max_vram_bytes: Some(10),
            ..Default::default()
        },
        ..unconstrained_req()
    };
    let res = agent.negotiate(req);
    assert_eq!(res.strategies.len(), 1);
    assert_eq!(res.strategies[0].id, StrategyId::LowPower);
}

#[test]
fn test_negotiate_strategy_ids_are_correct() {
    let mut agent = RenderAgent::default();
    let res = agent.negotiate(unconstrained_req());
    let ids: Vec<StrategyId> = res.strategies.iter().map(|s| s.id).collect();
    assert!(ids.contains(&StrategyId::LowPower));
    assert!(ids.contains(&StrategyId::Balanced));
    assert!(ids.contains(&StrategyId::HighPerformance));
}

#[test]
fn test_apply_budget_low_power_reflected_in_status() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget(StrategyId::LowPower));
    assert_eq!(agent.report_status().current_strategy, StrategyId::LowPower);
}

#[test]
fn test_apply_budget_balanced_reflected_in_status() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget(StrategyId::Balanced));
    assert_eq!(agent.report_status().current_strategy, StrategyId::Balanced);
}

#[test]
fn test_apply_budget_high_performance_reflected_in_status() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget(StrategyId::HighPerformance));
    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::HighPerformance
    );
}

#[test]
fn test_apply_budget_custom_records_strategy_id() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget(StrategyId::Custom(7)));
    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::Custom(7)
    );
}
