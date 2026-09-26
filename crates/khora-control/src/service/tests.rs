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

use super::forecast::forecast_total_ms;
use super::*;
use crate::cost_model::CostModel;
use crate::EngineMode;
use khora_core::control::gorna::{
    AdaptationMode, AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget,
    StrategyId, StrategyOption,
};
use khora_core::telemetry::{MetricId, MetricValue};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

struct StubAgent {
    applied: Option<StrategyId>,
}

impl Agent for StubAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }
    fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_millis(2),
                    estimated_vram: 1024,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_millis(8),
                    estimated_vram: 1024,
                },
            ],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.applied = Some(budget.strategy_id);
    }
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::Renderer,
            current_strategy: self.applied.unwrap_or(StrategyId::Balanced),
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, _: &mut khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[test]
fn test_dcc_service_lifecycle() {
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    dcc.start(rx);
    assert!(dcc.running.load(Ordering::SeqCst));
    dcc.stop();
    assert!(!dcc.running.load(Ordering::SeqCst));
}

#[test]
fn test_dcc_phase_change_ingestion() {
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::PhaseChange("Simulation".to_string()))
        .unwrap();

    thread::sleep(Duration::from_millis(100));

    let ctx = dcc.get_context();
    assert_eq!(ctx.mode, EngineMode::Playing);

    dcc.stop();
}

#[test]
fn test_dcc_metric_ingestion_smoke() {
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    let id = MetricId::new("test", "metric");
    tx.send(TelemetryEvent::MetricUpdate {
        id,
        value: MetricValue::Gauge(42.0),
    })
    .unwrap();

    thread::sleep(Duration::from_millis(50));
    dcc.stop();
}

#[test]
fn test_forecast_total_ms_sums_agent_models() {
    // No models yet → nothing to anticipate.
    let mut models: HashMap<AgentId, CostModel> = HashMap::new();
    assert!(forecast_total_ms(&models, 100.0, &[]).is_none());

    // Renderer: linear 3·n; Physics: linear 2·n.
    let mut renderer = CostModel::new(16);
    let mut physics = CostModel::new(16);
    for n in [10.0, 20.0, 40.0] {
        renderer.record(n, 3.0 * n);
        physics.record(n, 2.0 * n);
    }
    models.insert(AgentId::Renderer, renderer);
    models.insert(AgentId::Physics, physics);

    // Serial (empty plan): 300 + 200 = 500ms (within fit tolerance).
    let total = forecast_total_ms(&models, 100.0, &[]).expect("a fit is available");
    assert!((total - 500.0).abs() < 1.0, "serial forecast = {total}");

    // Concurrent wave [Renderer, Physics]: critical path = max(300, 200).
    let plan = vec![vec![AgentId::Renderer, AgentId::Physics]];
    let concurrent = forecast_total_ms(&models, 100.0, &plan).expect("a fit is available");
    assert!(
        (concurrent - 300.0).abs() < 1.0,
        "concurrent forecast = {concurrent}"
    );
}

#[test]
fn test_dcc_ingests_agent_cost_and_component_access() {
    // Smoke test for the observation-tunnel variants: the DCC must ingest
    // AgentCost + ComponentAccess without panicking and keep running.
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::AgentCost {
        id: AgentId::Renderer,
        n: 1000.0,
        time_ms: 4.2,
    })
    .unwrap();
    tx.send(TelemetryEvent::ComponentAccess {
        type_name: "Transform".to_string(),
        size_bytes: 40,
        query_count: 12,
        rows_scanned: 12_000,
    })
    .unwrap();

    thread::sleep(Duration::from_millis(50));
    assert!(dcc.running.load(Ordering::SeqCst));
    dcc.stop();
}

#[test]
fn test_dcc_layout_advisor_produces_recommendation() {
    // A lean component swept in large batches → the advisor recommends the
    // field-SoA/SIMD layout, surfaced read-only via `layout_recommendations`.
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::ComponentAccess {
        type_name: "Velocity".to_string(),
        size_bytes: 40,
        query_count: 10,
        rows_scanned: 40_960, // avg 4096 rows/query ≥ large-batch threshold
    })
    .unwrap();

    thread::sleep(Duration::from_millis(80));
    let recs = dcc.layout_recommendations();
    dcc.stop();

    assert_eq!(
        recs.get("Velocity"),
        Some(&LayoutRecommendation::SimdFieldSoa),
        "advisor should recommend field-SoA for a lean, large-batch component"
    );
}

#[test]
fn test_dcc_records_decisions() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let agent = Arc::new(std::sync::Mutex::new(StubAgent { applied: None }));
    dcc.register_agent(agent, 1.0);
    dcc.start_decision_recording();
    dcc.start(rx);

    thread::sleep(Duration::from_millis(150));
    let trace = dcc.stop_decision_recording();
    dcc.stop();

    assert!(
        !trace.ticks.is_empty(),
        "recording should capture at least one arbitration tick"
    );
    assert!(
        trace
            .ticks
            .iter()
            .all(|tick| tick.iter().any(|(id, _)| *id == AgentId::Renderer)),
        "every recorded tick should include the registered agent"
    );
}

#[test]
fn test_dcc_replay_flag_control() {
    let (dcc, _rx) = DccService::new(DccConfig::default());
    assert!(!dcc.is_replaying());

    let trace = DecisionTrace {
        ticks: vec![vec![(AgentId::Renderer, StrategyId::LowPower)]],
    };
    dcc.replay_decisions(trace);
    assert!(dcc.is_replaying());

    dcc.stop_replay();
    assert!(!dcc.is_replaying());
}

#[test]
fn test_dcc_initial_negotiation_fires_with_agent() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let agent = Arc::new(std::sync::Mutex::new(StubAgent { applied: None }));
    dcc.register_agent(agent.clone(), 1.0);
    dcc.start(rx);

    thread::sleep(Duration::from_millis(200));

    let applied = agent.lock().unwrap().applied.is_some();
    dcc.stop();

    assert!(
        applied,
        "Initial GORNA negotiation should have called apply_budget"
    );
}

#[test]
fn test_dcc_manual_mode_pins_strategy() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let agent = Arc::new(std::sync::Mutex::new(StubAgent { applied: None }));
    dcc.register_agent(agent.clone(), 1.0);
    // Developer pins the agent to LowPower. With ample budget, `Learning`
    // would otherwise pick Balanced (the most expensive offered strategy).
    dcc.set_adaptation_mode(
        AgentId::Renderer,
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    dcc.start(rx);

    thread::sleep(Duration::from_millis(200));

    let applied = agent.lock().unwrap().applied;
    dcc.stop();

    assert_eq!(
        applied,
        Some(StrategyId::LowPower),
        "Manual mode set via DccService should pin the agent's strategy"
    );
}

#[test]
fn test_pid_drives_multiplier_down_under_overrun() {
    // A sustained frame time well above the 16.66ms target must make the PID
    // loop pull the global budget multiplier below 1.0 over several ticks.
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);

    let frame_time_id = MetricId::new("renderer", "frame_time");
    // Feed a steady stream of 40ms frames (≫ the 16.66ms setpoint).
    for _ in 0..40 {
        tx.send(TelemetryEvent::MetricUpdate {
            id: frame_time_id.clone(),
            value: MetricValue::Gauge(40.0),
        })
        .unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(100));

    let multiplier = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        multiplier < 1.0,
        "sustained overrun should pull the multiplier below 1.0, got {multiplier}"
    );
    assert!(
        multiplier >= 0.3,
        "multiplier must respect the output floor, got {multiplier}"
    );
}

/// Stub whose two strategies straddle the frame budget: Balanced (14ms)
/// fits the 16.66ms target only when the budget multiplier is near 1.0,
/// so a degraded multiplier forces LowPower and a recovered one allows
/// the upgrade back — making the recovery path observable.
struct RecoveryStubAgent {
    applied: Option<StrategyId>,
}

impl Agent for RecoveryStubAgent {
    fn id(&self) -> AgentId {
        AgentId::Renderer
    }
    fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_millis(2),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_millis(14),
                    estimated_vram: 0,
                },
            ],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.applied = Some(budget.strategy_id);
    }
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::Renderer,
            current_strategy: self.applied.unwrap_or(StrategyId::LowPower),
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, _: &mut khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[test]
fn test_pid_recovery_reissues_budgets_and_upgrades() {
    // Once measured frame time settles back under the setpoint, the PID
    // multiplier climbs and its drift past PID_RENEGOTIATE_DELTA must
    // re-arbitrate so the agent is upgraded — without this trigger the
    // agent would stay pinned at the degraded strategy forever (no
    // pressure heuristic fires when everything is healthy).
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let agent = Arc::new(std::sync::Mutex::new(RecoveryStubAgent { applied: None }));
    dcc.register_agent(agent.clone(), 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);

    let frame_time_id = MetricId::new("renderer", "frame_time");

    // Phase 1 — sustained overrun (40ms ≫ 16.66ms): the PID pulls the
    // multiplier down until Balanced (14ms) no longer fits and the agent
    // is downgraded to LowPower. Poll instead of a fixed sleep.
    let mut degraded = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        tx.send(TelemetryEvent::MetricUpdate {
            id: frame_time_id.clone(),
            value: khora_core::telemetry::MetricValue::Gauge(40.0),
        })
        .unwrap();
        thread::sleep(Duration::from_millis(5));
        if agent.lock().unwrap().applied == Some(StrategyId::LowPower) {
            degraded = true;
            break;
        }
    }
    assert!(
        degraded,
        "sustained overrun should downgrade the agent to LowPower"
    );

    // Phase 2 — recovery (5ms ≪ 16.66ms): the averages drop below every
    // pressure threshold, so only the PID-drift trigger can re-issue
    // budgets. The multiplier climbs back and the agent must be upgraded.
    let mut recovered = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        tx.send(TelemetryEvent::MetricUpdate {
            id: frame_time_id.clone(),
            value: khora_core::telemetry::MetricValue::Gauge(5.0),
        })
        .unwrap();
        thread::sleep(Duration::from_millis(5));
        if agent.lock().unwrap().applied == Some(StrategyId::Balanced) {
            recovered = true;
            break;
        }
    }
    dcc.stop();

    assert!(
        recovered,
        "once frame time settles, the PID drift must re-arbitrate and upgrade the agent"
    );
}

#[test]
fn test_critical_thermal_clamps_multiplier() {
    // A Critical thermal report must clamp the multiplier to the hard safety
    // ceiling (0.4) immediately, regardless of the PID's current position.
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::HardwareReport(
        khora_core::telemetry::monitoring::HardwareReport {
            thermal: khora_core::platform::ThermalStatus::Critical,
            battery: khora_core::platform::BatteryLevel::Mains,
            cpu_load: 0.2,
            gpu_load: Some(0.2),
            gpu_timings: None,
        },
    ))
    .unwrap();
    thread::sleep(Duration::from_millis(120));

    let multiplier = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        multiplier <= 0.4 + 1e-3,
        "Critical thermal must clamp the multiplier to the safety ceiling, got {multiplier}"
    );
}
