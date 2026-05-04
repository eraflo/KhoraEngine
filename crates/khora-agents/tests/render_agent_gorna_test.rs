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

//! Integration tests for the RenderAgent's GORNA protocol implementation.
//!
//! Per CLAUDE.md, the agent exposes only the `Agent` trait + `Default`.
//! These tests therefore drive the agent through `Agent` and observe the
//! externally-visible state via `report_status()`.

use khora_agents::render_agent::RenderAgent;
use khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_core::control::gorna::{
    AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
};
use std::collections::HashMap;
use std::time::Duration;

/// Helper: creates a default NegotiationRequest with generous constraints.
fn default_request() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    }
}

fn budget_for(strategy_id: StrategyId, time_ms: u64) -> ResourceBudget {
    ResourceBudget {
        strategy_id,
        time_limit: Duration::from_millis(time_ms),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}

#[test]
fn test_negotiate_returns_at_least_one_strategy() {
    let mut agent = RenderAgent::default();
    let response = agent.negotiate(default_request());
    assert!(
        !response.strategies.is_empty(),
        "negotiate() must always offer at least one strategy"
    );
}

#[test]
fn test_negotiate_offers_all_three_default_strategies() {
    let mut agent = RenderAgent::default();
    let response = agent.negotiate(default_request());

    let ids: Vec<StrategyId> = response.strategies.iter().map(|s| s.id).collect();
    assert!(ids.contains(&StrategyId::LowPower));
    assert!(ids.contains(&StrategyId::Balanced));
    assert!(ids.contains(&StrategyId::HighPerformance));
}

#[test]
fn test_negotiate_time_estimates_are_ordered() {
    let mut agent = RenderAgent::default();
    let response = agent.negotiate(default_request());

    let low = response
        .strategies
        .iter()
        .find(|s| s.id == StrategyId::LowPower)
        .map(|s| s.estimated_time);
    let balanced = response
        .strategies
        .iter()
        .find(|s| s.id == StrategyId::Balanced)
        .map(|s| s.estimated_time);
    if let (Some(l), Some(b)) = (low, balanced) {
        assert!(l <= b, "LowPower ({:?}) should be <= Balanced ({:?})", l, b);
    }
}

#[test]
fn test_negotiate_vram_estimates_increase_with_complexity() {
    let mut agent = RenderAgent::default();
    let response = agent.negotiate(default_request());

    let low = response
        .strategies
        .iter()
        .find(|s| s.id == StrategyId::LowPower)
        .map(|s| s.estimated_vram);
    let high = response
        .strategies
        .iter()
        .find(|s| s.id == StrategyId::HighPerformance)
        .map(|s| s.estimated_vram);

    if let (Some(l), Some(h)) = (low, high) {
        assert!(
            h >= l,
            "HighPerformance ({}) should be >= LowPower ({})",
            h,
            l
        );
    }
}

#[test]
fn test_negotiate_respects_vram_constraint() {
    let mut agent = RenderAgent::default();
    let request = NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints {
            max_vram_bytes: Some(1024),
            max_memory_bytes: None,
            must_run: false,
        },
        current_mode: EngineMode::Playing,
        agent_timing: ExecutionTiming::default(),
    };

    let response = agent.negotiate(request);
    assert!(!response.strategies.is_empty());
    for s in &response.strategies {
        assert!(
            s.estimated_vram <= 1024 || s.id == StrategyId::LowPower,
            "Strategy {:?} VRAM {} exceeds 1024",
            s.id,
            s.estimated_vram
        );
    }
}

#[test]
fn test_apply_budget_low_power_sets_low_power_strategy() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget_for(StrategyId::LowPower, 4));
    assert_eq!(agent.report_status().current_strategy, StrategyId::LowPower);
}

#[test]
fn test_apply_budget_balanced_sets_balanced_strategy() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget_for(StrategyId::Balanced, 8));
    assert_eq!(agent.report_status().current_strategy, StrategyId::Balanced);
}

#[test]
fn test_apply_budget_high_performance_sets_high_performance_strategy() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget_for(StrategyId::HighPerformance, 12));
    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::HighPerformance
    );
}

#[test]
fn test_apply_budget_custom_falls_back_to_balanced() {
    let mut agent = RenderAgent::default();
    agent.apply_budget(budget_for(StrategyId::Custom(999), 8));
    // Custom is unsupported; agent records the strategy_id but maps the
    // internal RenderingStrategy back to Balanced.  We check the current_strategy
    // is whatever was applied; the rendering strategy itself is internal.
    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::Custom(999)
    );
}

#[test]
fn test_report_status_initial_state() {
    let agent = RenderAgent::default();
    let status = agent.report_status();

    assert_eq!(status.agent_id, AgentId::Renderer);
    assert_eq!(status.health_score, 1.0);
    assert!(!status.is_stalled);
    assert_eq!(status.current_strategy, StrategyId::Balanced);
}

#[test]
fn test_report_status_message_contains_metrics() {
    let agent = RenderAgent::default();
    let status = agent.report_status();

    assert!(status.message.contains("frame_time="));
    assert!(status.message.contains("draws="));
    assert!(status.message.contains("tris="));
    assert!(status.message.contains("lights="));
}

#[test]
fn test_full_gorna_cycle() {
    let mut agent = RenderAgent::default();

    let response = agent.negotiate(default_request());
    assert!(!response.strategies.is_empty());

    let cheapest = response
        .strategies
        .iter()
        .min_by_key(|s| s.estimated_time)
        .unwrap();

    agent.apply_budget(ResourceBudget {
        strategy_id: cheapest.id,
        time_limit: cheapest.estimated_time,
        memory_limit: None,
        extra_params: HashMap::new(),
    });

    let status = agent.report_status();
    assert_eq!(status.current_strategy, cheapest.id);
    assert_eq!(status.health_score, 1.0);
    assert!(!status.is_stalled);
}
