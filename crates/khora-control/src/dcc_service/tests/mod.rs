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

//! The DCC service: its thread, what it ingests, its forecast, its frame
//! governor, and the order it initializes agents in. The protocol it speaks
//! to the agents is in `protocol.rs`; when it opens an epoch, in `epochs.rs`;
//! how the governor drives the multiplier in `frame_governor.rs`, and the
//! hardware monitor its thread reads in `hardware_monitor.rs`; what it learns
//! of the machine, seeded and published, in `learnt_costs.rs`. The loop the
//! governor closes through GORNA, simulated, is pinned by the evaluation's
//! own tests.

mod epochs;
mod frame_governor;
mod hardware_monitor;
mod learnt_costs;
mod protocol;

use super::forecast::forecast_frame_ms;
use super::*;
use crate::cost_model::CostModel;
use khora_core::agent::gorna::protocol::{
    Allocation, Confidence, Envelope, Epoch, FrameReport, Offer, OfferRequest, OfferRow, Report,
    Trigger,
};
use khora_core::agent::gorna::{AgentId, EpochRecord, StrategyId};
use khora_core::agent::Place;
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
        why: khora_core::agent::gorna::protocol::Why::default(),
        importance: Vec::new(),
        corrections: Vec::new(),
        memory: PerDevice::default(),
        background: Vec::new(),
        at: Duration::ZERO,
        frames: khora_core::agent::gorna::FrameSummary::default(),
        in_force: Vec::new(),
        planning: khora_core::agent::gorna::Planning::default(),
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
        lever: false,
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
/// `floor` — and a `Balanced` row of `upgrade` when it has one — and shares
/// what it heard and how it is doing with the test.
struct Speaker {
    id: AgentId,
    floor: Duration,
    upgrade: Option<Duration>,
    heard: Arc<Mutex<Heard>>,
    health: Arc<Mutex<Health>>,
    /// A bare place: the DCC never runs one.
    place: Arc<Mutex<Place>>,
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
        Self::offering(name, floor, None)
    }

    /// `name`, offering a floor row of `floor` and a `Balanced` row of
    /// `upgrade` on the host: lowering the frame's target can take it off
    /// its upgrade.
    fn with_upgrade(
        name: &'static str,
        floor: Duration,
        upgrade: Duration,
    ) -> (Arc<Mutex<dyn Agent>>, Ear) {
        Self::offering(name, floor, Some(upgrade))
    }

    fn offering(
        name: &'static str,
        floor: Duration,
        upgrade: Option<Duration>,
    ) -> (Arc<Mutex<dyn Agent>>, Ear) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let health = Arc::new(Mutex::new(Health::default()));
        let agent: Arc<Mutex<dyn Agent>> = Arc::new(Mutex::new(Speaker {
            id: AgentId::new(name),
            floor,
            upgrade,
            heard: Arc::clone(&heard),
            health: Arc::clone(&health),
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
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

    /// The rows it offers: its floor, and its upgrade when it has one.
    fn rows(&self) -> Vec<OfferRow> {
        let mut rows = vec![self.floor_row()];
        if let Some(upgrade) = self.upgrade {
            rows.push(OfferRow {
                id: StrategyId::Balanced,
                cost: Cost {
                    time: PerDevice {
                        cpu: upgrade,
                        ..PerDevice::default()
                    },
                    ..Cost::default()
                },
                quality: 1.0,
                confidence: Confidence::Quoted,
            });
        }
        rows
    }
}

impl Agent for Speaker {
    fn id(&self) -> AgentId {
        self.id
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
            rows: self.rows(),
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
            message: None,
        }
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
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

/// **The DCC's context holds the families off**, as the frame boundary
/// last said.
#[test]
fn the_context_holds_the_families_off() {
    let (mut dcc, rx) = DccService::new(DccConfig::default());
    let tx = dcc.event_sender();
    dcc.start(rx);
    let off: std::collections::BTreeSet<AgentId> =
        [AgentId::new("script"), AgentId::new("physics")]
            .into_iter()
            .collect();

    tx.send(TelemetryEvent::ActivationChanged(off.clone()))
        .expect("the DCC listens");
    let held = wait_for(3, || dcc.get_context().inactive == off);
    dcc.stop();

    assert!(
        held,
        "the context's families off: {:?}",
        dcc.get_context().inactive
    );
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
    assert!(forecast_frame_ms(&models, &workloads, &[], &[]).is_none());

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
    let total = forecast_frame_ms(&models, &workloads, &[], &[]).expect("a fit is available");
    assert!((total - 500.0).abs() < 1.0, "serial forecast = {total}");

    // Concurrent wave [Renderer, Physics]: critical path = max(300, 200).
    let plan = vec![vec![AgentId::new("renderer"), AgentId::new("physics")]];
    let concurrent =
        forecast_frame_ms(&models, &workloads, &plan, &[]).expect("a fit is available");
    assert!(
        (concurrent - 300.0).abs() < 1.0,
        "concurrent forecast = {concurrent}"
    );
}

/// **The forecast leaves an agent at the background off the frame.** The
/// renderer costs 3·n and the oven 2·n, both at n = 100: the oven runs off
/// the frame, so the frame is forecast at the renderer's 300 ms, serial or
/// with the oven in the wave plan.
#[test]
fn the_forecast_leaves_an_agent_at_the_background_off_the_frame() {
    let mut renderer = CostModel::new(16);
    let mut oven = CostModel::new(16);
    for n in [10.0, 20.0, 40.0] {
        renderer.record(n, 3.0 * n);
        oven.record(n, 2.0 * n);
    }
    let models: HashMap<AgentId, CostModel> = [
        (AgentId::new("renderer"), renderer),
        (AgentId::new("oven"), oven),
    ]
    .into();
    let workloads: HashMap<AgentId, f64> = [
        (AgentId::new("renderer"), 100.0),
        (AgentId::new("oven"), 100.0),
    ]
    .into();
    let background = [AgentId::new("oven")];

    let serial =
        forecast_frame_ms(&models, &workloads, &[], &background).expect("a fit is available");
    assert!((serial - 300.0).abs() < 1.0, "serial forecast = {serial}");

    let planned = vec![vec![AgentId::new("renderer")], vec![AgentId::new("oven")]];
    let with_a_plan =
        forecast_frame_ms(&models, &workloads, &planned, &background).expect("a fit is available");
    assert!(
        (with_a_plan - 300.0).abs() < 1.0,
        "forecast with a plan = {with_a_plan}"
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

    let total = forecast_frame_ms(&models, &workloads, &[], &[]).expect("a fit is available");
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
        message: None,
    }
}

/// **Reports feed the cost models at their workload.** A run's workload is
/// the host time its row quoted, in µs: the renderer's runs quoted 2, 4, 8
/// then 16 ms and each took half as long again. Its model forecasts 24 ms at
/// its last workload, over the 16.66 ms frame: an epoch opens for the
/// forecast breach, and says why.
#[test]
fn reports_feed_the_cost_models_at_their_workload() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    for n in [2_000.0, 4_000.0, 8_000.0, 16_000.0] {
        tx.send(TelemetryEvent::AgentReport(run_report(
            "renderer",
            Epoch(0),
            (1.5 * n) as u64,
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

/// **A sustained overrun drives the multiplier down.** Frames of 40 ms, far
/// over the 16.66 ms target, the host and the GPU both busy the whole frame:
/// over several ticks the governor pulls the global budget multiplier below
/// 1.0, never under its floor.
#[test]
fn a_sustained_overrun_drives_the_multiplier_down() {
    let floor = DccConfig::default().frame_governor.output_min;
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);

    // Feed a steady stream of 40ms frames (≫ the 16.66ms setpoint), the host
    // and the GPU both busy for the whole frame.
    for _ in 0..40 {
        tx.send(TelemetryEvent::FrameReport(frame_report(40.0, Some(40.0))))
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
        multiplier >= floor,
        "multiplier must respect the output floor {floor}, got {multiplier}"
    );
}

// ── The frame report ──────────────────────────────────────────────

/// A frame report: the host busy `cpu_ms`, the GPU `gpu_ms` when the frame
/// measured it; no other device measured.
fn frame_report(cpu_ms: f64, gpu_ms: Option<f64>) -> FrameReport {
    let ms = |ms: f64| Duration::from_secs_f64(ms / 1_000.0);
    FrameReport {
        busy: PerDevice {
            cpu: Some(ms(cpu_ms)),
            gpu: gpu_ms.map(ms),
            npu: None,
            other: None,
        },
    }
}

/// A DCC ticking at 100 Hz, started, and the sender it listens on.
fn listening() -> (DccService, crossbeam_channel::Sender<TelemetryEvent>) {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    (dcc, tx)
}

/// Sends `event` every 5 ms until the multiplier drops below 1.0 or `secs`
/// seconds have passed; the multiplier it dropped to, if it did.
fn feed_until_the_multiplier_drops(
    dcc: &DccService,
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
    secs: u64,
    event: impl Fn() -> TelemetryEvent,
) -> Option<f32> {
    let dropped = wait_for(secs, || {
        tx.send(event()).expect("the DCC listens");
        dcc.get_context().global_budget_multiplier < 1.0
    });
    dropped.then(|| dcc.get_context().global_budget_multiplier)
}

/// Sends `event` `count` times, 5 ms apart — many more samples than the DCC
/// waits for (`FRAME_TIME_MIN_SAMPLES`), over many DCC ticks — then waits a
/// few ticks more; the lowest multiplier seen meanwhile.
fn lowest_multiplier_while_feeding(
    dcc: &DccService,
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
    count: usize,
    event: impl Fn() -> TelemetryEvent,
) -> f32 {
    assert!(count >= 4 * FRAME_TIME_MIN_SAMPLES);
    let mut lowest = dcc.get_context().global_budget_multiplier;
    for _ in 0..count {
        tx.send(event()).expect("the DCC listens");
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    lowest
}

/// **A host-bound frame pulls the multiplier down without GPU timing.**
/// Frames keeping the host busy 40 ms, the GPU not measured, are frames of
/// 40 ms: the governor pulls the multiplier below 1.0, never under its 0.3
/// floor.
#[test]
fn a_host_bound_frame_without_gpu_timing_pulls_the_multiplier_down() {
    let (mut dcc, tx) = listening();

    let dropped = feed_until_the_multiplier_drops(&dcc, &tx, 5, || {
        TelemetryEvent::FrameReport(frame_report(40.0, None))
    });
    dcc.stop();

    let multiplier = dropped.expect("40 ms frames pull the multiplier below 1.0");
    assert!(multiplier >= 0.3, "the output floor holds: {multiplier}");
}

/// **A GPU-bound frame pulls the multiplier down.** The host busy 5 ms, the
/// GPU 40 ms: the frame's critical path is the GPU's, over the 16.66 ms
/// target.
#[test]
fn a_gpu_bound_frame_pulls_the_multiplier_down() {
    let (mut dcc, tx) = listening();

    let dropped = feed_until_the_multiplier_drops(&dcc, &tx, 5, || {
        TelemetryEvent::FrameReport(frame_report(5.0, Some(40.0)))
    });
    dcc.stop();

    assert!(
        dropped.is_some(),
        "the GPU's 40 ms is the frame's critical path"
    );
}

/// **The critical path is the slower device, not the sum.** The host busy
/// 10 ms and the GPU 9 ms work side by side: the frame lasts 10 ms, within
/// the 16.66 ms target, and the multiplier stays at 1.0 — though the two
/// added up (19 ms) would overrun it. The DCC was listening: frames of 40 ms
/// then pull the multiplier down.
#[test]
fn the_critical_path_is_the_slower_device_not_the_sum() {
    let (mut dcc, tx) = listening();

    let lowest = lowest_multiplier_while_feeding(&dcc, &tx, 60, || {
        TelemetryEvent::FrameReport(frame_report(10.0, Some(9.0)))
    });
    let control = feed_until_the_multiplier_drops(&dcc, &tx, 5, || {
        TelemetryEvent::FrameReport(frame_report(40.0, None))
    });
    dcc.stop();

    assert!(
        lowest >= 1.0,
        "a 10 ms critical path leaves the multiplier at 1.0, lowest {lowest}"
    );
    assert!(
        control.is_some(),
        "the DCC reads frame reports: 40 ms frames pull the multiplier down"
    );
}

/// **The renderer's frame-time metric no longer moves the multiplier.** A
/// steady `renderer:frame_time` of 40 ms leaves it at 1.0: the frame's time
/// comes from its report. The DCC was listening: frame reports of 40 ms then
/// pull the multiplier down.
#[test]
fn the_renderer_frame_time_metric_no_longer_moves_the_multiplier() {
    let (mut dcc, tx) = listening();

    let lowest = lowest_multiplier_while_feeding(&dcc, &tx, 60, || TelemetryEvent::MetricUpdate {
        id: MetricId::new("renderer", "frame_time"),
        value: MetricValue::Gauge(40.0),
    });
    let control = feed_until_the_multiplier_drops(&dcc, &tx, 5, || {
        TelemetryEvent::FrameReport(frame_report(40.0, None))
    });
    dcc.stop();

    assert!(
        lowest >= 1.0,
        "a renderer:frame_time of 40 ms leaves the multiplier at 1.0, lowest {lowest}"
    );
    assert!(
        control.is_some(),
        "the DCC reads frame reports: 40 ms frames pull the multiplier down"
    );
}

/// **Too few frames move nothing.** One report fewer than the DCC waits for
/// (`FRAME_TIME_MIN_SAMPLES`), each of a 200 ms frame, leaves the multiplier
/// at 1.0 over many DCC ticks: a lone hitch is not a trend. The next report
/// completes the window and the governor pulls the multiplier down.
#[test]
fn fewer_frame_reports_than_the_dcc_waits_for_move_nothing() {
    let (mut dcc, tx) = listening();
    let hitch = || TelemetryEvent::FrameReport(frame_report(200.0, None));

    for _ in 0..FRAME_TIME_MIN_SAMPLES - 1 {
        tx.send(hitch()).expect("the DCC listens");
    }
    let mut lowest = dcc.get_context().global_budget_multiplier;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    tx.send(hitch()).expect("the DCC listens");
    let control = wait_for(5, || dcc.get_context().global_budget_multiplier < 1.0);
    dcc.stop();

    assert!(
        lowest >= 1.0,
        "{} frame reports leave the multiplier at 1.0, lowest {lowest}",
        FRAME_TIME_MIN_SAMPLES - 1
    );
    assert!(
        control,
        "the report completing the window moves the governor"
    );
}

// ── The frame governor ────────────────────────────────────────────

/// The frame-time target the DCC regulates on with no thermal, battery or
/// memory pressure: the heuristics' setpoint for a default context.
fn unpressured_setpoint_ms() -> f32 {
    crate::analysis::HeuristicEngine
        .analyze(
            &crate::dcc_context::Context::default(),
            &crate::metrics::MetricStore::new(),
        )
        .suggested_latency_ms
}

/// The frame time whose estimate lowers the multiplier from 1.0 to 0.75 in
/// one decision: the governor's target (the setpoint less its headroom)
/// over 0.75. Under twice the setpoint — load, never a hitch — and under the
/// heuristics' 18 ms warning, so no other trigger joins in.
fn frame_ms_lowering_by_a_quarter() -> f64 {
    let governor = DccConfig::default().frame_governor;
    f64::from(unpressured_setpoint_ms() * (1.0 - governor.headroom) / 0.75)
}

/// The multiplier clearly off the governor's floor: a quarter of its range
/// above it.
fn clearly_off_the_floor() -> f32 {
    let governor = DccConfig::default().frame_governor;
    governor.output_min + 0.25 * (governor.output_max - governor.output_min)
}

/// Sends `count` frames of `frame_ms` at once.
fn burst(tx: &crossbeam_channel::Sender<TelemetryEvent>, count: usize, frame_ms: f64) {
    for _ in 0..count {
        tx.send(TelemetryEvent::FrameReport(frame_report(frame_ms, None)))
            .expect("the DCC listens");
    }
}

/// Sends, at once, as many frames as the DCC waits for, each lowering the
/// multiplier by a quarter, then waits for the multiplier to drop; the
/// multiplier it dropped to, if it did. All of them reach the governor
/// before it decides, so it decides once, on frames of one length.
fn lowered_by_a_quarter(
    dcc: &DccService,
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
) -> Option<f32> {
    burst(tx, FRAME_TIME_MIN_SAMPLES, frame_ms_lowering_by_a_quarter());
    let dropped = wait_for(2, || dcc.get_context().global_budget_multiplier < 1.0);
    thread::sleep(Duration::from_millis(50));
    dropped.then(|| dcc.get_context().global_budget_multiplier)
}

/// **A silence holds the multiplier.** Frames over the target pull the
/// multiplier down; once the reports stop, it stays where the last frames
/// left it — read twice a second apart, it has not moved — off its floor and
/// below 1.0. The governor decides on the frames it observed: a tick that
/// received none is no new measurement.
#[test]
fn a_silence_holds_the_multiplier_where_the_last_frames_left_it() {
    let (mut dcc, tx) = listening();

    let dropped = lowered_by_a_quarter(&dcc, &tx);
    thread::sleep(Duration::from_millis(200));
    let first = dcc.get_context().global_budget_multiplier;
    thread::sleep(Duration::from_secs(1));
    let second = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        dropped.is_some(),
        "frames of {:.1} ms pull the multiplier below 1.0",
        frame_ms_lowering_by_a_quarter()
    );
    assert!(
        (first - second).abs() < 1e-3,
        "with no frame reported, the multiplier holds: {first} then {second} a second later"
    );
    assert!(first < 1.0, "the multiplier stays down: {first}");
    assert!(
        first > clearly_off_the_floor(),
        "the multiplier held off its floor: {first}"
    );
}

/// **The multiplier leaves its floor as soon as frames run fast again.**
/// Frames of 200 ms pin the multiplier at its floor, and go on for eight
/// seconds of frame time after it — longer than the governor remembers the
/// multiplier it lowered from. Then frames of 5 ms, well under the target:
/// within two seconds the multiplier is clearly off its floor. The governor
/// reads the frames it observes: the metric store's window, still full of
/// 200 ms frames, would hold it far above the target for as many frames
/// again.
#[test]
fn the_multiplier_leaves_its_floor_as_soon_as_frames_run_fast_again() {
    let governor = DccConfig::default().frame_governor;
    let (mut dcc, tx) = listening();

    let floored = wait_for(5, || {
        burst(&tx, 1, 200.0);
        dcc.get_context().global_budget_multiplier <= governor.output_min + 1e-3
    });
    for _ in 0..40 {
        burst(&tx, 1, 200.0);
        thread::sleep(Duration::from_millis(5));
    }
    let recovered = wait_for(2, || {
        burst(&tx, 5, 5.0);
        dcc.get_context().global_budget_multiplier > clearly_off_the_floor()
    });
    let multiplier = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(floored, "200 ms frames pin the multiplier at its floor");
    assert!(
        recovered,
        "5 ms frames lift the multiplier above {} within two seconds, got {multiplier}",
        clearly_off_the_floor()
    );
}

/// A frame report that measured no device: its critical path is unknown.
fn unmeasured_frame() -> TelemetryEvent {
    TelemetryEvent::FrameReport(FrameReport {
        busy: PerDevice {
            cpu: None,
            gpu: None,
            npu: None,
            other: None,
        },
    })
}

/// Sends `event` every 5 ms for `millis` milliseconds.
fn feed_for(
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
    millis: u64,
    event: impl Fn() -> TelemetryEvent,
) {
    let deadline = std::time::Instant::now() + Duration::from_millis(millis);
    while std::time::Instant::now() < deadline {
        tx.send(event()).expect("the DCC listens");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A hardware report of `thermal` on mains power.
fn thermal_report(thermal: khora_core::platform::ThermalStatus) -> TelemetryEvent {
    TelemetryEvent::HardwareReport(khora_core::telemetry::monitoring::HardwareReport {
        thermal,
        battery: khora_core::platform::BatteryLevel::Mains,
        cpu_load: 0.2,
        gpu_load: Some(0.2),
    })
}

/// **A frame that measured nothing is not a frame of zero.** Each burst is
/// one 40 ms frame and nine reports that measured no device: the frames the
/// governor observes are the 40 ms ones alone, and they pull the multiplier
/// down. Counted as frames of 0 ms, the nine would come between the 40 ms
/// frames.
#[test]
fn reports_that_measured_no_device_are_not_frames() {
    let (mut dcc, tx) = listening();

    let dropped = wait_for(5, || {
        burst(&tx, 1, 40.0);
        for _ in 0..9 {
            tx.send(unmeasured_frame()).expect("the DCC listens");
        }
        dcc.get_context().global_budget_multiplier < 1.0
    });
    dcc.stop();

    assert!(
        dropped,
        "the 40 ms frames pull the multiplier down, the unmeasured reports aside"
    );
}

/// **Frames of zero duration hold the multiplier.** Once frames over the
/// target have pulled it down, a second of frames measured at 0 ms leaves it
/// where it was: a frame of 0 ms is no measurement the governor acts on.
#[test]
fn frames_of_zero_duration_hold_the_multiplier() {
    let (mut dcc, tx) = listening();

    let dropped = lowered_by_a_quarter(&dcc, &tx);
    thread::sleep(Duration::from_millis(200));
    let first = dcc.get_context().global_budget_multiplier;
    feed_for(&tx, 1_000, || {
        TelemetryEvent::FrameReport(frame_report(0.0, None))
    });
    thread::sleep(Duration::from_millis(50));
    let second = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        dropped.is_some(),
        "the frames over the target pull the multiplier down"
    );
    assert!(
        (first - second).abs() < 1e-3,
        "frames of 0 ms hold the multiplier: {first} then {second}"
    );
}

/// **A silence does not become one large step.** The multiplier was lowered
/// by a quarter, then frames of 14 ms — inside the governor's band — hold
/// it. After a second without frames, one frame of 30 ms: it weighs in the
/// estimate as one frame of 30 ms, not as a second of them, and the
/// multiplier barely moves.
#[test]
fn a_silence_does_not_become_one_large_step() {
    let (mut dcc, tx) = listening();

    let dropped = lowered_by_a_quarter(&dcc, &tx);
    burst(&tx, 200, 14.0);
    thread::sleep(Duration::from_millis(200));
    let before = dcc.get_context().global_budget_multiplier;
    thread::sleep(Duration::from_secs(1));
    burst(&tx, 1, 30.0);
    thread::sleep(Duration::from_millis(100));
    let after = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        dropped.is_some(),
        "the frames over the target pull the multiplier down"
    );
    assert!(
        (before - after).abs() < 0.05,
        "one frame after a silence moves the multiplier one frame's worth: {before} then {after}"
    );
}

/// **The safety ceiling lifts back to the held multiplier.** With no frame
/// reported, a Critical thermal state caps the multiplier at 0.4; once the
/// device cools, the multiplier is the governor's held one again, neither
/// the cap nor 1.0.
#[test]
fn a_held_multiplier_comes_back_once_the_safety_ceiling_lifts() {
    use khora_core::platform::ThermalStatus;
    let (mut dcc, tx) = listening();

    let dropped = lowered_by_a_quarter(&dcc, &tx);
    thread::sleep(Duration::from_millis(200));
    let held = dcc.get_context().global_budget_multiplier;
    tx.send(thermal_report(ThermalStatus::Critical))
        .expect("the DCC listens");
    let capped = wait_for(2, || {
        dcc.get_context().global_budget_multiplier <= 0.4 + 1e-3
    });
    tx.send(thermal_report(ThermalStatus::Cool))
        .expect("the DCC listens");
    let back = wait_for(2, || {
        (dcc.get_context().global_budget_multiplier - held).abs() < 1e-3
    });
    let multiplier = dcc.get_context().global_budget_multiplier;
    dcc.stop();

    assert!(
        dropped.is_some(),
        "the frames over the target pull the multiplier down"
    );
    assert!(held > 0.4 + 0.05, "held above the cap: {held}");
    assert!(capped, "Critical thermal caps the multiplier at 0.4");
    assert!(
        back,
        "once cool, the multiplier is the held {held} again, got {multiplier}"
    );
}

/// **A long frame leaves the loop once it has passed.** Frames of 10 ms,
/// well under the target, then one frame stalled ten seconds (a blocking
/// load, a debugger break), then frames of 10 ms again: over the second of
/// fast frames that follows, the multiplier stays within 0.05 of its rest.
/// The long frame follows frames within the setpoint: a hitch, kept out of
/// the governor's estimate.
#[test]
fn a_single_long_frame_does_not_pin_the_multiplier_once_it_has_passed() {
    let fast = || TelemetryEvent::FrameReport(frame_report(10.0, None));
    let (mut dcc, tx) = listening();

    feed_for(&tx, 150, fast);
    thread::sleep(Duration::from_millis(50));
    let at_rest = dcc.get_context().global_budget_multiplier;
    burst(&tx, 1, 10_000.0);
    let mut lowest = at_rest;
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        tx.send(fast()).expect("the DCC listens");
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    dcc.stop();

    assert!(
        at_rest >= 1.0,
        "10 ms frames leave the multiplier at rest: {at_rest}"
    );
    assert!(
        lowest > 1.0 - 0.05,
        "a ten-second frame among 10 ms ones moves the multiplier less than 0.05, lowest {lowest}"
    );
}

/// **A lone hitch is not a trend.** Among frames of 10 ms, one frame of
/// 100 ms: the multiplier stays at rest. The hitch follows a frame within
/// the setpoint, so the governor keeps it out of its estimate.
#[test]
fn a_lone_hitch_among_fast_frames_leaves_the_multiplier_near_rest() {
    let fast = || TelemetryEvent::FrameReport(frame_report(10.0, None));
    let (mut dcc, tx) = listening();

    feed_for(&tx, 150, fast);
    thread::sleep(Duration::from_millis(50));
    burst(&tx, 1, 100.0);
    let mut lowest = dcc.get_context().global_budget_multiplier;
    for _ in 0..60 {
        tx.send(fast()).expect("the DCC listens");
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    dcc.stop();

    assert!(
        lowest >= 1.0,
        "one 100 ms frame among 10 ms ones leaves the multiplier at 1.0, lowest {lowest}"
    );
}

#[test]
fn test_critical_thermal_clamps_multiplier() {
    // A Critical thermal report must clamp the multiplier to the hard safety
    // ceiling (0.4) immediately, regardless of the governor's current position.
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
