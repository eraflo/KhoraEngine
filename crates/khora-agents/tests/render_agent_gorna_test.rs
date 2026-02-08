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
//! These tests exercise the negotiate → apply_budget → report_status cycle
//! to verify the RenderAgent integrates correctly with the DCC.

use khora_agents::render_agent::{RenderAgent, RenderingStrategy};
use khora_core::{
    agent::Agent,
    control::gorna::{
        AgentId, NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId,
    },
};
use std::collections::HashMap;
use std::time::Duration;

/// Helper: creates a default NegotiationRequest with generous constraints.
fn default_request() -> NegotiationRequest {
    NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// negotiate() tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_negotiate_returns_at_least_one_strategy() {
    let mut agent = RenderAgent::new();
    let response = agent.negotiate(default_request());
    assert!(
        !response.strategies.is_empty(),
        "negotiate() must always offer at least one strategy"
    );
}

#[test]
fn test_negotiate_offers_all_three_default_lanes() {
    let mut agent = RenderAgent::new();
    let response = agent.negotiate(default_request());

    let ids: Vec<StrategyId> = response.strategies.iter().map(|s| s.id).collect();
    assert!(ids.contains(&StrategyId::LowPower), "Should offer LowPower");
    assert!(ids.contains(&StrategyId::Balanced), "Should offer Balanced");
    assert!(
        ids.contains(&StrategyId::HighPerformance),
        "Should offer HighPerformance"
    );
}

#[test]
fn test_negotiate_time_estimates_are_ordered() {
    let mut agent = RenderAgent::new();
    let response = agent.negotiate(default_request());

    let times: Vec<(StrategyId, Duration)> = response
        .strategies
        .iter()
        .map(|s| (s.id, s.estimated_time))
        .collect();

    // LowPower should cost less than or equal to Balanced
    let low = times.iter().find(|(id, _)| *id == StrategyId::LowPower);
    let balanced = times.iter().find(|(id, _)| *id == StrategyId::Balanced);
    if let (Some(l), Some(b)) = (low, balanced) {
        assert!(
            l.1 <= b.1,
            "LowPower time ({:?}) should be <= Balanced ({:?})",
            l.1,
            b.1
        );
    }
}

#[test]
fn test_negotiate_vram_estimates_increase_with_complexity() {
    let mut agent = RenderAgent::new();
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
            "HighPerformance VRAM ({}) should be >= LowPower ({})",
            h,
            l
        );
    }
}

#[test]
fn test_negotiate_respects_vram_constraint() {
    let mut agent = RenderAgent::new();

    // Set VRAM constraint very low — should filter out heavy strategies.
    let request = NegotiationRequest {
        target_latency: Duration::from_millis(16),
        priority_weight: 1.0,
        constraints: ResourceConstraints {
            max_vram_bytes: Some(1024), // 1KB — only LowPower should fit
            max_memory_bytes: None,
            must_run: false,
        },
    };

    let response = agent.negotiate(request);
    // Should still have at least 1 strategy (guaranteed fallback).
    assert!(!response.strategies.is_empty());
    // All offered strategies must respect the VRAM constraint.
    for s in &response.strategies {
        assert!(
            s.estimated_vram <= 1024 || s.id == StrategyId::LowPower,
            "Strategy {:?} has VRAM {} which exceeds constraint 1024",
            s.id,
            s.estimated_vram
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// apply_budget() tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_apply_budget_switches_strategy() {
    let mut agent = RenderAgent::new();
    assert_eq!(agent.strategy(), RenderingStrategy::Auto);

    let budget = ResourceBudget {
        strategy_id: StrategyId::LowPower,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: HashMap::new(),
    };
    agent.apply_budget(budget);

    assert_eq!(agent.strategy(), RenderingStrategy::Unlit);
    assert_eq!(agent.current_strategy_id(), StrategyId::LowPower);
}

#[test]
fn test_apply_budget_preserves_all_lanes() {
    let mut agent = RenderAgent::new();
    let initial_lanes = agent.lanes().len();
    assert_eq!(initial_lanes, 3, "Should start with 3 default lanes");

    // Apply budget — lanes should NOT be destroyed.
    let budget = ResourceBudget {
        strategy_id: StrategyId::LowPower,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: HashMap::new(),
    };
    agent.apply_budget(budget);

    assert_eq!(
        agent.lanes().len(),
        initial_lanes,
        "apply_budget() must not destroy lanes"
    );
}

#[test]
fn test_apply_budget_then_switch_back_to_auto() {
    let mut agent = RenderAgent::new();

    // GORNA assigns LowPower
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::LowPower,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: HashMap::new(),
    });
    assert_eq!(agent.strategy(), RenderingStrategy::Unlit);

    // User manually sets back to Auto
    agent.set_strategy(RenderingStrategy::Auto);
    assert_eq!(agent.strategy(), RenderingStrategy::Auto);

    // Should still be able to select lanes (they weren't destroyed)
    let lane = agent.select_lane();
    assert!(
        ["SimpleUnlit", "LitForward", "ForwardPlus"].contains(&lane.strategy_name()),
        "Auto mode should still be functional"
    );
}

#[test]
fn test_apply_budget_custom_fallback_to_balanced() {
    let mut agent = RenderAgent::new();

    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Custom(999),
        time_limit: Duration::from_millis(8),
        memory_limit: None,
        extra_params: HashMap::new(),
    });

    assert_eq!(
        agent.strategy(),
        RenderingStrategy::LitForward,
        "Custom strategy should fallback to LitForward"
    );
}

#[test]
fn test_apply_budget_stores_time_budget() {
    let mut agent = RenderAgent::new();

    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: Duration::from_millis(8),
        memory_limit: None,
        extra_params: HashMap::new(),
    });

    // After apply_budget, the time_limit feeds into report_status health.
    let status = agent.report_status();
    // With no frames rendered yet, health should be 1.0.
    assert_eq!(
        status.health_score, 1.0,
        "Health should be 1.0 with no frames rendered"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// report_status() tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_report_status_initial_state() {
    let agent = RenderAgent::new();
    let status = agent.report_status();

    assert_eq!(status.agent_id, AgentId::Renderer);
    assert_eq!(status.health_score, 1.0); // No frames → healthy
    assert!(!status.is_stalled); // No device → not stalled
    assert_eq!(status.current_strategy, StrategyId::Balanced);
}

#[test]
fn test_report_status_message_contains_metrics() {
    let agent = RenderAgent::new();
    let status = agent.report_status();

    assert!(
        status.message.contains("frame_time="),
        "Message should include frame_time"
    );
    assert!(
        status.message.contains("draws="),
        "Message should include draw call count"
    );
    assert!(
        status.message.contains("tris="),
        "Message should include triangle count"
    );
    assert!(
        status.message.contains("lights="),
        "Message should include light count"
    );
}

#[test]
fn test_report_status_reflects_strategy_change() {
    let mut agent = RenderAgent::new();

    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::HighPerformance,
        time_limit: Duration::from_millis(16),
        memory_limit: None,
        extra_params: HashMap::new(),
    });

    let status = agent.report_status();
    assert_eq!(status.current_strategy, StrategyId::HighPerformance);
}

// ─────────────────────────────────────────────────────────────────────────────
// Full negotiate → apply → status cycle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_gorna_cycle() {
    let mut agent = RenderAgent::new();

    // 1. DCC negotiates with the agent
    let response = agent.negotiate(default_request());
    assert!(!response.strategies.is_empty());

    // 2. DCC picks the cheapest strategy
    let cheapest = response
        .strategies
        .iter()
        .min_by_key(|s| s.estimated_time)
        .unwrap();

    // 3. DCC sends the budget
    agent.apply_budget(ResourceBudget {
        strategy_id: cheapest.id,
        time_limit: cheapest.estimated_time,
        memory_limit: None,
        extra_params: HashMap::new(),
    });

    // 4. Agent reports status
    let status = agent.report_status();
    assert_eq!(status.current_strategy, cheapest.id);
    assert_eq!(status.health_score, 1.0); // No frames yet
    assert!(!status.is_stalled);
}

#[test]
fn test_negotiate_then_switch_strategy_preserves_lanes() {
    let mut agent = RenderAgent::new();

    // Cycle through all strategies
    for strategy_id in [
        StrategyId::LowPower,
        StrategyId::Balanced,
        StrategyId::HighPerformance,
        StrategyId::Balanced,
        StrategyId::LowPower,
    ] {
        agent.apply_budget(ResourceBudget {
            strategy_id,
            time_limit: Duration::from_millis(8),
            memory_limit: None,
            extra_params: HashMap::new(),
        });

        // Lanes should never shrink
        assert_eq!(
            agent.lanes().len(),
            3,
            "All 3 lanes must survive strategy switching"
        );

        // Selected lane should match the strategy
        let expected = match strategy_id {
            StrategyId::LowPower => "SimpleUnlit",
            StrategyId::Balanced => "LitForward",
            StrategyId::HighPerformance => "ForwardPlus",
            _ => unreachable!(),
        };
        assert_eq!(agent.select_lane().strategy_name(), expected);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Render metrics accessors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_frame_count_starts_at_zero() {
    let agent = RenderAgent::new();
    assert_eq!(agent.frame_count(), 0);
    assert_eq!(agent.last_frame_time(), Duration::ZERO);
}

// ─────────────────────────────────────────────────────────────────────────────
// Telemetry sender integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_telemetry_sender_can_be_attached() {
    use crossbeam_channel::unbounded;

    let (tx, rx) = unbounded();
    let mut agent = RenderAgent::new().with_telemetry_sender(tx);

    // Negotiate + apply to drive the agent state.
    let response = agent.negotiate(default_request());
    assert!(!response.strategies.is_empty());

    let budget = ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: Duration::from_millis(16),
        memory_limit: None,
        extra_params: HashMap::new(),
    };
    agent.apply_budget(budget);

    // The channel is wired; no events yet since update() hasn't been called.
    // But the receiver proves the wiring is complete.
    assert!(
        rx.try_recv().is_err(),
        "No events should be emitted before update()"
    );
}
