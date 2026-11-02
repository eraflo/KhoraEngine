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

//! The DCC service: its thread, what it ingests, its forecast, its PID, and
//! the order it initializes agents in. The protocol it speaks to the agents
//! is in `protocol.rs`; when it opens an epoch, in `epochs.rs`.

mod epochs;
mod protocol;

use super::forecast::forecast_frame_ms;
use super::*;
use crate::cost_model::CostModel;
use crate::EngineMode;
use khora_core::agent::gorna::protocol::{
    Allocation, Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report, Trigger,
};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, EpochRecord, NegotiationRequest, NegotiationResponse, ResourceBudget,
    StrategyId, StrategyOption,
};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::telemetry::{MetricId, MetricValue};
use std::sync::atomic::Ordering;
use std::time::Duration;

// ── Fakes and waiting ─────────────────────────────────────────────

/// Waits up to `secs` seconds for `done`, polling every 5 ms.
fn wait_for(secs: u64, mut done: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    done()
}

/// A DCC ticking at 100 Hz, recording its epochs, with `config` otherwise.
fn recording(config: DccConfig) -> (DccService, Receiver<TelemetryEvent>) {
    let (dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..config
    });
    dcc.start_decision_recording();
    (dcc, rx)
}

/// The allocations the DCC has issued so far.
fn epochs(dcc: &DccService) -> Vec<Allocation> {
    records(dcc)
        .into_iter()
        .map(|record| record.allocation)
        .collect()
}

/// The epochs the DCC has journaled so far, inputs and all.
fn records(dcc: &DccService) -> Vec<EpochRecord> {
    dcc.recorded_decisions().epochs
}

/// A journal entry holding `allocation` and nothing of its inputs, as a test
/// writes a trace to replay.
fn record(allocation: Allocation) -> EpochRecord {
    EpochRecord {
        allocation,
        target: Duration::from_micros(16_667),
        offers: Vec::new(),
        reports: Vec::new(),
        waves: Vec::new(),
        policies: Vec::new(),
        hints: Vec::new(),
        reasons: Vec::new(),
    }
}

/// An allocation for `epoch`, opened by `trigger`, lending each named agent
/// its envelope.
fn allocation(epoch: u64, trigger: Trigger, envelopes: Vec<(AgentId, Envelope)>) -> Allocation {
    Allocation {
        epoch: Epoch(epoch),
        trigger,
        envelopes,
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        late: Vec::new(),
    }
}

/// An envelope of `micros` on the host.
fn cpu_envelope(micros: u64) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Envelope::default()
    }
}

/// A DCC and a scheduler over the same agents, as the engine runs them: the
/// DCC publishes its allocations on the scheduler's channel, and the
/// scheduler, run by the test on its own thread, lends them at its frame
/// starts and publishes each agent's report to the DCC.
struct Host {
    dcc: DccService,
    rx: Option<Receiver<TelemetryEvent>>,
    scheduler: Option<crate::scheduler::ExecutionScheduler>,
    world: khora_data::ecs::World,
    runtime: Arc<khora_core::Runtime>,
}

impl Host {
    /// A DCC of `config`, recording its epochs; no scheduler yet.
    fn new(config: DccConfig) -> Self {
        let (dcc, rx) = DccService::new(config);
        dcc.start_decision_recording();
        Self {
            dcc,
            rx: Some(rx),
            scheduler: None,
            world: khora_data::ecs::World::new(),
            runtime: Arc::new(khora_core::Runtime::new()),
        }
    }

    /// Builds the scheduler over the DCC's agents, connects the two, and
    /// starts the DCC's thread.
    fn start(&mut self) {
        let mut scheduler = crate::scheduler::ExecutionScheduler::from_registry(
            Arc::clone(self.dcc.agent_registry()),
            self.dcc.context_handle(),
        );
        scheduler.set_telemetry_sender(self.dcc.event_sender());
        self.dcc
            .connect_budget_channel(scheduler.budget_channel().clone());
        self.scheduler = Some(scheduler);
        let rx = self.rx.take().expect("the host starts once");
        self.dcc.start(rx);
    }

    /// Runs frames on this thread, one every 2 ms, until `done` says so of
    /// the DCC or `secs` seconds have passed; whether `done` came.
    fn frames_until(&mut self, secs: u64, mut done: impl FnMut(&DccService) -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        while std::time::Instant::now() < deadline {
            if let Some(scheduler) = self.scheduler.as_mut() {
                scheduler.run_frame(&mut self.world, Arc::clone(&self.runtime));
            }
            if done(&self.dcc) {
                return true;
            }
            thread::sleep(Duration::from_millis(2));
        }
        done(&self.dcc)
    }
}

/// Whether some issued allocation was opened by `trigger`.
fn opened_by(dcc: &DccService, trigger: Trigger) -> bool {
    epochs(dcc).iter().any(|epoch| epoch.trigger == trigger)
}

/// What a [`Speaker`] heard from the DCC.
#[derive(Debug, Default)]
struct Heard {
    asked: Vec<OfferRequest>,
    lent: Vec<(Epoch, Envelope, Option<StrategyId>)>,
    /// The thread each `apply` came on, in order.
    lent_on: Vec<thread::ThreadId>,
    /// Calls to `negotiate` or `apply_budget`, which a protocol agent never
    /// expects.
    old_calls: usize,
}

/// How a [`Speaker`] is doing; the test changes it while the DCC runs.
#[derive(Debug)]
struct Health {
    health: f32,
    stalled: bool,
    /// How many of its next reports say over budget (`u32::MAX`: all).
    over_budget_reports: u32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            health: 1.0,
            stalled: false,
            over_budget_reports: 0,
        }
    }
}

/// An agent that speaks the protocol: it offers one `LowPower` row of
/// `floor`, and shares what it heard and how it is doing with the test.
struct Speaker {
    id: AgentId,
    floor: Duration,
    heard: Arc<Mutex<Heard>>,
    health: Arc<Mutex<Health>>,
}

/// The test's side of a [`Speaker`].
struct Ear {
    heard: Arc<Mutex<Heard>>,
    health: Arc<Mutex<Health>>,
}

impl Ear {
    fn heard(&self) -> std::sync::MutexGuard<'_, Heard> {
        self.heard.lock().expect("the heard lock")
    }

    fn health(&self) -> std::sync::MutexGuard<'_, Health> {
        self.health.lock().expect("the health lock")
    }
}

impl Speaker {
    /// `name`, offering one row of `floor` on the host.
    fn with_ear(name: &'static str, floor: Duration) -> (Arc<Mutex<dyn Agent>>, Ear) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let health = Arc::new(Mutex::new(Health::default()));
        let agent: Arc<Mutex<dyn Agent>> = Arc::new(Mutex::new(Speaker {
            id: AgentId::new(name),
            floor,
            heard: Arc::clone(&heard),
            health: Arc::clone(&health),
        }));
        (agent, Ear { heard, health })
    }

    fn floor_row(&self) -> OfferRow {
        OfferRow {
            id: StrategyId::LowPower,
            cost: Cost {
                time: PerDevice {
                    cpu: self.floor,
                    ..PerDevice::default()
                },
                ..Cost::default()
            },
            quality: 0.4,
            confidence: Confidence::Quoted,
        }
    }
}

impl Agent for Speaker {
    fn id(&self) -> AgentId {
        self.id
    }
    fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
        self.heard.lock().expect("the heard lock").old_calls += 1;
        NegotiationResponse {
            strategies: Vec::new(),
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _: ResourceBudget) {
        self.heard.lock().expect("the heard lock").old_calls += 1;
    }
    fn report_status(&self) -> AgentStatus {
        let health = self.health.lock().expect("the health lock");
        AgentStatus {
            agent_id: self.id,
            current_strategy: StrategyId::LowPower,
            health_score: health.health,
            is_stalled: health.stalled,
            message: String::new(),
        }
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        self.heard
            .lock()
            .expect("the heard lock")
            .asked
            .push(request.clone());
        Offer {
            agent: self.id,
            epoch: request.epoch,
            rows: vec![self.floor_row()],
            floor: StrategyId::LowPower,
            current: StrategyId::LowPower,
            health: self.health.lock().expect("the health lock").health,
        }
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: &Envelope,
        pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        let mut heard = self.heard.lock().expect("the heard lock");
        heard.lent.push((epoch, *envelope, pin));
        heard.lent_on.push(thread::current().id());
    }
    fn report(&self) -> Report {
        let mut health = self.health.lock().expect("the health lock");
        let over_budget = health.over_budget_reports > 0;
        if health.over_budget_reports != u32::MAX {
            health.over_budget_reports = health.over_budget_reports.saturating_sub(1);
        }
        let epoch = self
            .heard
            .lock()
            .expect("the heard lock")
            .lent
            .last()
            .map_or(Epoch::default(), |(epoch, _, _)| *epoch);
        Report {
            agent: self.id,
            epoch,
            row: StrategyId::LowPower,
            measured: PerDevice {
                cpu: self.floor,
                ..PerDevice::default()
            },
            done: 1.0,
            health: health.health,
            over_budget,
            stalled: health.stalled,
            workload: None,
            switch: None,
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

/// An agent written before the protocol, answering through the bridge: two
/// options, `LowPower` 2 ms and `Balanced` 8 ms.
struct StubAgent {
    applied: Option<StrategyId>,
}

impl Agent for StubAgent {
    fn id(&self) -> AgentId {
        AgentId::new("renderer")
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
            agent_id: AgentId::new("renderer"),
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
fn test_dcc_mode_change_ingestion() {
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "Editor".to_string(),
    )))
    .unwrap();

    thread::sleep(Duration::from_millis(100));

    let ctx = dcc.get_context();
    assert_eq!(ctx.mode, EngineMode::Custom("Editor".to_string()));

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

/// **The forecast sums the agents' models on the critical path**: serial, a
/// frame costs every model at its workload; in one wave, the slowest.
#[test]
fn the_forecast_sums_agent_models_on_the_critical_path() {
    // No models yet → nothing to anticipate.
    let mut models: HashMap<AgentId, CostModel> = HashMap::new();
    let mut workloads: HashMap<AgentId, f64> = HashMap::new();
    assert!(forecast_frame_ms(&models, &workloads, &[]).is_none());

    // Renderer: linear 3·n; Physics: linear 2·n; both at n = 100.
    let mut renderer = CostModel::new(16);
    let mut physics = CostModel::new(16);
    for n in [10.0, 20.0, 40.0] {
        renderer.record(n, 3.0 * n);
        physics.record(n, 2.0 * n);
    }
    models.insert(AgentId::new("renderer"), renderer);
    models.insert(AgentId::new("physics"), physics);
    workloads.insert(AgentId::new("renderer"), 100.0);
    workloads.insert(AgentId::new("physics"), 100.0);

    // Serial (empty plan): 300 + 200 = 500ms (within fit tolerance).
    let total = forecast_frame_ms(&models, &workloads, &[]).expect("a fit is available");
    assert!((total - 500.0).abs() < 1.0, "serial forecast = {total}");

    // Concurrent wave [Renderer, Physics]: critical path = max(300, 200).
    let plan = vec![vec![AgentId::new("renderer"), AgentId::new("physics")]];
    let concurrent = forecast_frame_ms(&models, &workloads, &plan).expect("a fit is available");
    assert!(
        (concurrent - 300.0).abs() < 1.0,
        "concurrent forecast = {concurrent}"
    );
}

/// **Each agent is forecast at its own workload.** The renderer last drew
/// 100 items (3·n = 300 ms), physics last stepped 10 bodies (2·n = 20 ms):
/// 320 ms. One workload shared by every agent would forecast both at
/// whichever agent reported last — 500 ms or 50 ms.
#[test]
fn each_agent_is_forecast_at_its_own_workload() {
    let mut renderer = CostModel::new(16);
    let mut physics = CostModel::new(16);
    for n in [10.0, 20.0, 40.0] {
        renderer.record(n, 3.0 * n);
        physics.record(n, 2.0 * n);
    }
    let models: HashMap<AgentId, CostModel> = [
        (AgentId::new("renderer"), renderer),
        (AgentId::new("physics"), physics),
    ]
    .into();
    let workloads: HashMap<AgentId, f64> = [
        (AgentId::new("renderer"), 100.0),
        (AgentId::new("physics"), 10.0),
    ]
    .into();

    let total = forecast_frame_ms(&models, &workloads, &[]).expect("a fit is available");
    assert!((total - 320.0).abs() < 1.0, "forecast = {total}");
}

/// A report of `name`'s run on `epoch`: `cpu_us` on the host at `workload`,
/// on its `LowPower` row, healthy, within budget.
fn run_report(name: &'static str, epoch: Epoch, cpu_us: u64, workload: f64) -> Report {
    Report {
        agent: AgentId::new(name),
        epoch,
        row: StrategyId::LowPower,
        measured: PerDevice {
            cpu: Duration::from_micros(cpu_us),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(workload),
        switch: None,
    }
}

/// **Reports feed the cost models at their workload.** The renderer's runs
/// cost 0.3 ms per unit of work; its last run did 80 units. Its model
/// forecasts 24 ms at that workload, over the 16.66 ms frame: an epoch opens
/// for the forecast breach, and says why.
#[test]
fn reports_feed_the_cost_models_at_their_workload() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    for n in [10.0, 20.0, 40.0, 80.0] {
        tx.send(TelemetryEvent::AgentReport(run_report(
            "renderer",
            Epoch(0),
            (300.0 * n) as u64,
            n,
        )))
        .expect("the DCC listens");
    }
    dcc.start(rx);

    let breached = wait_for(3, || opened_by(&dcc, Trigger::ForecastBreach));
    let journaled = records(&dcc);
    dcc.stop();

    assert!(breached, "{:?}", epochs_of(&journaled));
    let breach = journaled
        .iter()
        .find(|record| record.allocation.trigger == Trigger::ForecastBreach)
        .expect("the breach epoch");
    assert!(
        !breach.reasons.is_empty(),
        "a breach epoch says why it opened"
    );
}

/// The allocations of `records`, for a failure message.
fn epochs_of(records: &[EpochRecord]) -> Vec<&Allocation> {
    records.iter().map(|record| &record.allocation).collect()
}

#[test]
fn test_dcc_ingests_agent_reports_and_component_access() {
    // Smoke test for the observation-tunnel variants: the DCC must ingest
    // AgentReport + ComponentAccess without panicking and keep running.
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::AgentReport(run_report(
        "renderer",
        Epoch(0),
        4_200,
        1000.0,
    )))
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

/// An agent that writes its name into a shared log when it is initialized.
/// Each has its own id: the DCC holds one agent per id.
struct InitRecording {
    name: &'static str,
    id: AgentId,
    log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
}

impl Agent for InitRecording {
    fn id(&self) -> AgentId {
        self.id
    }
    fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn on_initialize(&mut self, _: &mut khora_core::EngineContext<'_>) {
        self.log.lock().expect("the log lock").push(self.name);
    }
    fn execute(&mut self, _: &mut khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A DCC holding three recording agents registered out of priority order,
/// and the log they write to.
fn dcc_with_three_recording_agents() -> (
    DccService,
    std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
) {
    let (dcc, _rx) = DccService::new(DccConfig::default());
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for (name, priority, id) in [
        ("low", 1.0, AgentId::new("script")),
        ("high", 3.0, AgentId::new("audio")),
        ("middle", 2.0, AgentId::new("ui")),
    ] {
        dcc.register_agent(
            std::sync::Arc::new(std::sync::Mutex::new(InitRecording {
                name,
                id,
                log: std::sync::Arc::clone(&log),
            })),
            priority,
        );
    }
    (dcc, log)
}

#[test]
fn initialize_agent_over_every_index_initializes_each_agent_once_in_priority_order() {
    let (dcc, log) = dcc_with_three_recording_agents();
    let runtime = std::sync::Arc::new(khora_core::Runtime::new());
    let bus = khora_core::lane::LaneBus::new();

    for index in 0..dcc.agent_count() {
        let mut deck = khora_core::lane::OutputDeck::new();
        let mut ctx = khora_core::EngineContext::for_initialisation(
            std::sync::Arc::clone(&runtime),
            &bus,
            &mut deck,
        );
        assert!(
            dcc.initialize_agent(index, &mut ctx),
            "index {index} names a registered agent"
        );
    }

    assert_eq!(
        *log.lock().expect("the log lock"),
        vec!["high", "middle", "low"],
        "every agent initialized once, highest priority first — as initialize_agents does"
    );
}

#[test]
fn initialize_agent_past_the_last_agent_returns_false_and_initializes_nothing() {
    let (dcc, log) = dcc_with_three_recording_agents();
    let bus = khora_core::lane::LaneBus::new();
    let mut deck = khora_core::lane::OutputDeck::new();
    let mut ctx = khora_core::EngineContext::for_initialisation(
        std::sync::Arc::new(khora_core::Runtime::new()),
        &bus,
        &mut deck,
    );

    assert!(!dcc.initialize_agent(dcc.agent_count(), &mut ctx));
    assert!(
        log.lock().expect("the log lock").is_empty(),
        "no agent was initialized"
    );
}

#[test]
fn initialize_agent_leaves_the_later_agents_uninitialized() {
    let (dcc, log) = dcc_with_three_recording_agents();
    let bus = khora_core::lane::LaneBus::new();
    let mut deck = khora_core::lane::OutputDeck::new();
    let mut ctx = khora_core::EngineContext::for_initialisation(
        std::sync::Arc::new(khora_core::Runtime::new()),
        &bus,
        &mut deck,
    );

    assert!(dcc.initialize_agent(0, &mut ctx));
    assert_eq!(*log.lock().expect("the log lock"), vec!["high"]);
}

/// **A hint that is not a usable number is refused**: a weight or a cap that
/// is not a number, or an infinite weight, is not folded into the agent's
/// hints, and the hints already set stay.
#[test]
fn a_hint_that_is_not_a_number_is_refused() {
    use khora_core::agent::gorna::EngineHint;
    let (dcc, _rx) = DccService::new(DccConfig::default());
    let renderer = AgentId::new("renderer");
    dcc.set_hint(EngineHint::Cap {
        agent: renderer,
        max_ms: 12.0,
    });

    dcc.set_hint(EngineHint::Prioritize {
        agent: renderer,
        weight: f32::NAN,
    });
    dcc.set_hint(EngineHint::Prioritize {
        agent: renderer,
        weight: f32::INFINITY,
    });
    dcc.set_hint(EngineHint::Cap {
        agent: renderer,
        max_ms: f32::NAN,
    });

    let hints = dcc.hints().get(&renderer).copied().unwrap_or_default();
    assert_eq!(hints.priority, None);
    assert_eq!(hints.cap_ms, Some(12.0));
}
