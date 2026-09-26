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

use super::*;
use crate::analysis::AnalysisReport;
use crate::context::Context;
use crate::EngineMode;
use khora_core::agent::Agent;
use khora_core::control::gorna::{
    AdaptationMode, AgentHints, AgentId, AgentStatus, NegotiationRequest, NegotiationResponse,
    ResourceBudget, StrategyId, StrategyOption, TickDecisions,
};
use khora_core::EngineContext;

// ── Mock Agent ───────────────────────────────────────────────────

struct MockAgent {
    id: AgentId,
    applied_budget: Option<ResourceBudget>,
    is_stalled: bool,
    health: f32,
}

impl MockAgent {
    fn new(id: AgentId) -> Self {
        Self {
            id,
            applied_budget: None,
            is_stalled: false,
            health: 1.0,
        }
    }

    fn stalled(id: AgentId) -> Self {
        Self {
            id,
            applied_budget: None,
            is_stalled: true,
            health: 0.0,
        }
    }
}

impl Agent for MockAgent {
    fn id(&self) -> AgentId {
        self.id
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
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
                    estimated_vram: 10 * 1024 * 1024,
                },
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_millis(14),
                    estimated_vram: 20 * 1024 * 1024,
                },
            ],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.applied_budget = Some(budget);
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id,
            current_strategy: self
                .applied_budget
                .as_ref()
                .map(|b| b.strategy_id)
                .unwrap_or(StrategyId::Balanced),
            health_score: self.health,
            is_stalled: self.is_stalled,
            message: String::new(),
        }
    }

    fn execute(&mut self, _context: &mut EngineContext<'_>) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn normal_report() -> AnalysisReport {
    AnalysisReport {
        needs_negotiation: true,
        suggested_latency_ms: 16.66,
        death_spiral_detected: false,
        alerts: Vec::new(),
    }
}

fn simulation_ctx() -> Context {
    Context {
        mode: EngineMode::Playing,
        global_budget_multiplier: 1.0,
        ..Default::default()
    }
}

// ── Tests ────────────────────────────────────────────────────────

fn create_arbitrator() -> GornaArbitrator {
    GornaArbitrator::new(Duration::from_millis(100))
}

#[test]
fn test_measured_costs_calibrate_fit_downward() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    // The agent quotes Balanced at 8ms but actually measured 24ms (×3).
    // Calibration rescales the options to 6/24/42ms, so within the 16.66ms
    // budget only LowPower fits — the fit must downgrade instead of
    // trusting the optimistic quote (which would have picked HighPerformance).
    let measured: HashMap<AgentId, f64> = [(AgentId::Renderer, 24.0)].into();
    let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &measured, None, &HashMap::new());

    assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
}

#[test]
fn test_calibration_factor_is_clamped() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    // A pathological measurement (800ms vs the 8ms quote = ×100) is clamped
    // to ×4: options become 8/32/56ms. Even the cheapest exceeds nothing —
    // LowPower (8ms) still fits the 16.66ms budget, but no upgrade does.
    let measured: HashMap<AgentId, f64> = [(AgentId::Renderer, 800.0)].into();
    let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &measured, None, &HashMap::new());

    assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
}

#[test]
fn test_arbitrate_single_agent_gets_best_strategy() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    let budget = mock
        .applied_budget
        .as_ref()
        .expect("Budget should be applied");
    // With 16.66ms total budget and a single agent, it should get HighPerformance (14ms)
    assert_eq!(budget.strategy_id, StrategyId::HighPerformance);
}

#[test]
fn test_fit_budgets_costs_concurrent_wave_by_critical_path() {
    let ctx = simulation_ctx();
    let report = normal_report();
    let two_agents = || -> Vec<Arc<Mutex<dyn Agent>>> {
        vec![
            Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer))),
            Arc::new(Mutex::new(MockAgent::new(AgentId::Physics))),
        ]
    };

    // Serial (no wave plan): 14 + 14ms > 16.66ms budget, so the fit cannot
    // grant both agents HighPerformance — one is downgraded. This is the
    // pre-parallel sum-of-costs behaviour, unchanged.
    let serial = create_arbitrator();
    let mut agents = two_agents();
    let issued = serial.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );
    let hp_serial = issued
        .iter()
        .filter(|(_, s)| *s == StrategyId::HighPerformance)
        .count();
    assert!(
        hp_serial < 2,
        "serial fit must not grant both HighPerformance: {issued:?}"
    );

    // Concurrent wave [Renderer, Physics]: the wave costs max(14, 14) = 14ms
    // on the critical path, which fits the budget — so both reach
    // HighPerformance. This is the win parallel execution unlocks.
    let mut concurrent = create_arbitrator();
    concurrent.set_wave_plan(&[vec![AgentId::Renderer, AgentId::Physics]]);
    let mut agents = two_agents();
    let issued = concurrent.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );
    let hp_concurrent = issued
        .iter()
        .filter(|(_, s)| *s == StrategyId::HighPerformance)
        .count();
    assert_eq!(
        hp_concurrent, 2,
        "concurrent wave must grant both HighPerformance: {issued:?}"
    );
}

#[test]
fn test_manual_mode_pins_strategy_against_budget() {
    let mut arbitrator = create_arbitrator();
    arbitrator.set_adaptation_mode(
        AgentId::Renderer,
        AdaptationMode::Manual(StrategyId::LowPower),
    );

    let ctx = simulation_ctx();
    let report = normal_report();
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    let budget = mock
        .applied_budget
        .as_ref()
        .expect("Budget should be applied");
    // The same 16.66ms budget yields HighPerformance under `Learning` (test
    // above). `Manual` pins the developer's choice instead: LowPower.
    assert_eq!(budget.strategy_id, StrategyId::LowPower);
}

#[test]
fn test_stable_mode_blocks_opportunistic_upgrade() {
    let mut arbitrator = create_arbitrator();
    arbitrator.set_adaptation_mode(AgentId::Renderer, AdaptationMode::Stable);

    let ctx = simulation_ctx();
    let report = normal_report();
    // MockAgent reports `Balanced` as its current strategy until a budget is
    // applied. With a 16.66ms budget the fit would upgrade to HighPerformance,
    // but `Stable` forbids opportunistic upgrades — it stays at Balanced.
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::Balanced
    );
}

#[test]
fn test_bounded_mode_clamps_to_max() {
    let mut arbitrator = create_arbitrator();
    arbitrator.set_adaptation_mode(
        AgentId::Renderer,
        AdaptationMode::Bounded {
            min: StrategyId::LowPower,
            max: StrategyId::Balanced,
        },
    );

    let ctx = simulation_ctx();
    let report = normal_report();
    // Fit would pick HighPerformance; bounds cap it at Balanced.
    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::Balanced
    );
}

#[test]
fn test_arbitrate_respects_global_budget() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();

    // Two agents: Renderer (priority 1.0) and Physics (priority 1.0)
    // Total budget: 16.66ms
    // Each agent offers: LowPower(2ms), Balanced(8ms), HighPerformance(14ms)
    // Both can't be HighPerformance (14+14=28ms > 16.66ms)
    // With priority-based allocation, they should get strategies that fit.
    let renderer = MockAgent::new(AgentId::Renderer);
    let physics = MockAgent::new(AgentId::Physics);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
        Arc::new(Mutex::new(renderer)),
        Arc::new(Mutex::new(physics)),
    ];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    // Both should have received budgets
    for agent_mutex in &agents {
        let lock = agent_mutex.lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        assert!(mock.applied_budget.is_some());
    }

    // Total cost should not exceed 16.66ms
    let total_cost_ms: f64 = agents
        .iter()
        .map(|a| {
            let lock = a.lock().unwrap();
            let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
            mock.applied_budget
                .as_ref()
                .unwrap()
                .time_limit
                .as_secs_f64()
                * 1000.0
        })
        .sum();
    assert!(
        total_cost_ms <= 16.66 + 0.1,
        "Total cost {:.2}ms exceeds budget 16.66ms",
        total_cost_ms
    );
}

#[test]
fn test_arbitrate_thermal_reduces_budget() {
    let arbitrator = create_arbitrator();
    let mut ctx = simulation_ctx();
    ctx.hardware.thermal = khora_core::platform::ThermalStatus::Throttling;
    // The PID owns the multiplier in the live loop; here we pin it directly
    // to exercise the lever `arbitrate` consumes.
    ctx.global_budget_multiplier = 0.6;

    let mut report = normal_report();
    report.suggested_latency_ms = 33.33; // Heuristic suggestion for throttling

    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    let budget = mock
        .applied_budget
        .as_ref()
        .expect("Budget should be applied");
    // Effective budget: 33.33 * 0.6 = ~20ms. Agent can easily get HighPerformance (14ms).
    assert_eq!(budget.strategy_id, StrategyId::HighPerformance);
}

#[test]
fn test_emergency_stop_on_death_spiral() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let mut report = normal_report();
    report.death_spiral_detected = true;

    let renderer = MockAgent::new(AgentId::Renderer);
    let physics = MockAgent::new(AgentId::Physics);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
        Arc::new(Mutex::new(renderer)),
        Arc::new(Mutex::new(physics)),
    ];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    // Both agents should be forced to LowPower
    for agent_mutex in &agents {
        let lock = agent_mutex.lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        let budget = mock
            .applied_budget
            .as_ref()
            .expect("Budget should be applied");
        assert_eq!(budget.strategy_id, StrategyId::LowPower);
    }
}

#[test]
fn test_emergency_stop_on_stalled_agents() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();

    // Two stalled agents should trigger emergency stop
    let stalled1 = MockAgent::stalled(AgentId::Renderer);
    let stalled2 = MockAgent::stalled(AgentId::Physics);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
        Arc::new(Mutex::new(stalled1)),
        Arc::new(Mutex::new(stalled2)),
    ];

    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    // Both should be forced to LowPower
    for agent_mutex in &agents {
        let lock = agent_mutex.lock().unwrap();
        let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
        let budget = mock
            .applied_budget
            .as_ref()
            .expect("Budget should be applied");
        assert_eq!(budget.strategy_id, StrategyId::LowPower);
    }
}

#[test]
fn test_arbitrate_empty_agents() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![];

    // Should not panic
    arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );
}

#[test]
fn test_priority_order_renderer_before_asset_in_simulation() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();

    // Tight budget: only 10ms total. Renderer (priority 1.0) should be
    // upgraded before Asset (priority 0.5).
    let mut tight_report = report;
    tight_report.suggested_latency_ms = 10.0;

    let renderer = MockAgent::new(AgentId::Renderer);
    let asset = MockAgent::new(AgentId::Asset);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(renderer)), Arc::new(Mutex::new(asset))];

    arbitrator.arbitrate(
        &ctx,
        &tight_report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    // With 10ms total: both minimum = 2+2=4ms, remaining=6ms.
    // Renderer (priority 1.0) should be upgraded first: +6ms → Balanced (8ms).
    // Asset (priority 0.5) stays at LowPower (2ms). Total: 8+2=10ms ≤ 10ms.
    let renderer_lock = agents[0].lock().unwrap();
    let renderer_mock = unsafe { &*((&*renderer_lock as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        renderer_mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::Balanced
    );
}

#[test]
fn test_hint_cap_clamps_issued_strategy() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

    // Ample budget would fit HighPerformance (14ms), but a 5ms Cap leaves
    // only LowPower (2ms) within the ceiling — Balanced (8ms) exceeds it.
    let hints: HashMap<AgentId, AgentHints> = [(
        AgentId::Renderer,
        AgentHints {
            cap_ms: Some(5.0),
            priority: None,
        },
    )]
    .into();
    let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
    assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::LowPower)]);
}

#[test]
fn test_hint_cap_allows_richest_within_ceiling() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

    // A 10ms Cap admits Balanced (8ms) but not HighPerformance (14ms).
    let hints: HashMap<AgentId, AgentHints> = [(
        AgentId::Renderer,
        AgentHints {
            cap_ms: Some(10.0),
            priority: None,
        },
    )]
    .into();
    let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
    assert_eq!(issued, vec![(AgentId::Renderer, StrategyId::Balanced)]);
}

#[test]
fn test_hint_prioritize_reorders_budget_fit() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut tight_report = report;
    tight_report.suggested_latency_ms = 10.0;

    // Default priorities upgrade Renderer (1.0) before Asset (0.5). A
    // Prioritize hint lifting Asset above Renderer flips the fit: Asset
    // takes the single available upgrade to Balanced, Renderer stays LowPower.
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![
        Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer))),
        Arc::new(Mutex::new(MockAgent::new(AgentId::Asset))),
    ];
    let hints: HashMap<AgentId, AgentHints> = [(
        AgentId::Asset,
        AgentHints {
            cap_ms: None,
            priority: Some(2.0),
        },
    )]
    .into();

    arbitrator.arbitrate(
        &ctx,
        &tight_report,
        &mut agents,
        &HashMap::new(),
        None,
        &hints,
    );

    let renderer = agents[0].lock().unwrap();
    let renderer_mock = unsafe { &*((&*renderer as *const dyn Agent) as *const MockAgent) };
    let asset = agents[1].lock().unwrap();
    let asset_mock = unsafe { &*((&*asset as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        asset_mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::Balanced,
        "the prioritized agent takes the upgrade"
    );
    assert_eq!(
        renderer_mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::LowPower,
        "the deprioritized agent is downgraded"
    );
}

#[test]
fn test_hint_targets_only_its_agent() {
    // A Cap on a DIFFERENT agent must not touch Renderer: with ample budget
    // and no Renderer hint, it still reaches HighPerformance (regression-0).
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];
    let hints: HashMap<AgentId, AgentHints> = [(
        AgentId::Audio,
        AgentHints {
            cap_ms: Some(1.0),
            priority: None,
        },
    )]
    .into();
    let issued = arbitrator.arbitrate(&ctx, &report, &mut agents, &HashMap::new(), None, &hints);
    assert_eq!(
        issued,
        vec![(AgentId::Renderer, StrategyId::HighPerformance)]
    );
}

#[test]
fn test_arbitrate_returns_issued_decisions() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

    let issued = arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );
    // Single agent, ample budget → HighPerformance, reported back as issued.
    assert_eq!(
        issued,
        vec![(AgentId::Renderer, StrategyId::HighPerformance)]
    );
}

#[test]
fn test_replay_overrides_fit_with_recorded_decision() {
    let arbitrator = create_arbitrator();
    let ctx = simulation_ctx();
    let report = normal_report();
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> =
        vec![Arc::new(Mutex::new(MockAgent::new(AgentId::Renderer)))];

    // A recorded decision of LowPower must be issued verbatim, even though
    // the live fit (ample budget) would pick HighPerformance.
    let recorded: TickDecisions = vec![(AgentId::Renderer, StrategyId::LowPower)];
    let replayed = arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        Some(&recorded),
        &HashMap::new(),
    );
    assert_eq!(replayed, vec![(AgentId::Renderer, StrategyId::LowPower)]);

    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::LowPower
    );
}

#[test]
fn test_vram_budget_caps_upgrade() {
    let arbitrator = create_arbitrator();
    let mut ctx = simulation_ctx();
    let report = normal_report();

    // Ample time budget (16.66ms) would let a lone agent reach
    // HighPerformance (14ms). But HighPerformance costs 20MB of VRAM,
    // Balanced 10MB. Cap available VRAM at 15MB: the fit must stop at
    // Balanced — the time budget is not the binding constraint here.
    ctx.hardware.available_vram = Some(15 * 1024 * 1024);

    let agent = MockAgent::new(AgentId::Renderer);
    let mut agents: Vec<Arc<Mutex<dyn Agent>>> = vec![Arc::new(Mutex::new(agent))];

    let issued = arbitrator.arbitrate(
        &ctx,
        &report,
        &mut agents,
        &HashMap::new(),
        None,
        &HashMap::new(),
    );

    assert_eq!(
        issued,
        vec![(AgentId::Renderer, StrategyId::Balanced)],
        "VRAM ceiling must block the HighPerformance upgrade"
    );
    let lock = agents[0].lock().unwrap();
    let mock = unsafe { &*((&*lock as *const dyn Agent) as *const MockAgent) };
    assert_eq!(
        mock.applied_budget.as_ref().unwrap().strategy_id,
        StrategyId::Balanced
    );
}

#[test]
fn test_agent_priorities() {
    let arbitrator = create_arbitrator();
    assert!(arbitrator.get_agent_priority(AgentId::Renderer) >= 0.9);
    assert!(arbitrator.get_agent_priority(AgentId::Physics) >= 0.9);
    assert!(arbitrator.get_agent_priority(AgentId::Ui) >= 0.5);
    assert!(arbitrator.get_agent_priority(AgentId::Audio) >= 0.5);
}

#[test]
fn test_critical_agents() {
    let arbitrator = create_arbitrator();
    assert!(arbitrator.is_critical_agent(AgentId::Renderer));
    assert!(arbitrator.is_critical_agent(AgentId::Physics));
    assert!(arbitrator.is_critical_agent(AgentId::Ecs));
    assert!(arbitrator.is_critical_agent(AgentId::Ui));
    assert!(!arbitrator.is_critical_agent(AgentId::Audio));
}
