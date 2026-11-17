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

//! Starting and stopping the DCC's decision thread — the cold-path loop that
//! ingests telemetry, runs the heuristics and re-arbitrates budgets.

use super::epochs::Epochs;
use super::forecast::forecast_frame_ms;
use super::{connected, DccConfig, DccService, FRAME_TIME_MIN_SAMPLES, MULTIPLIER_EPSILON};
use crate::agent_registry::AgentRegistry;
use crate::analysis::{AnalysisReport, HeuristicEngine};
use crate::budget_channel::BudgetChannel;
use crate::cost_model::CostModel;
use crate::dcc_context::{safety_ceiling, DccContext};
use crate::gorna::GornaArbitrator;
use crate::governor::FrameGovernor;
use crate::metrics::MetricStore;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::protocol::{Allocation, Envelope, Report, Trigger};
use khora_core::agent::gorna::{
    AdaptationMode, AgentHints, AgentId, DecisionTrace, DecisionWatch, EpochRecord, FrameSummary,
    ProfileRow,
};
use khora_core::agent::Agent;
use khora_core::platform::HardwareMonitor;
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::telemetry::{MetricId, TelemetryEvent};
use khora_data::ecs::layout::{LayoutAdvisor, LayoutRecommendation};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

/// How often the DCC's thread reads its hardware monitor.
const HARDWARE_READ_PERIOD: Duration = Duration::from_secs(1);

impl DccService {
    /// Starts the DCC's decision thread; does nothing if it already runs.
    ///
    /// See the book: concepts/the-frame.md, "Two clocks, one channel".
    pub fn start(&mut self, event_rx: Receiver<TelemetryEvent>) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        let shared = Shared {
            running: Arc::clone(&self.running),
            context: Arc::clone(&self.context),
            registry: Arc::clone(&self.registry),
            budget_channel: Arc::clone(&self.budget_channel),
            adaptation_modes: Arc::clone(&self.adaptation_modes),
            hints: Arc::clone(&self.hints),
            layout_recommendations: Arc::clone(&self.layout_recommendations),
            decision_recording: Arc::clone(&self.decision_recording),
            recorded_trace: Arc::clone(&self.recorded_trace),
            seeded_costs: Arc::clone(&self.seeded_costs),
            learnt_costs: Arc::clone(&self.learnt_costs),
            decision_watch: self.decision_watch.clone(),
            replay: Arc::clone(&self.replay),
            hardware_monitor: Arc::clone(&self.hardware_monitor),
        };
        let config = self.config.clone();
        #[expect(
            clippy::disallowed_methods,
            reason = "RULES §5: the DCC tick thread is the concurrency authority, spawned once at start"
        )]
        let handle = thread::spawn(move || DecisionLoop::new(shared, config).run(&event_rx));

        self.handle = Some(handle);
    }

    /// Stops the DCC background thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// What the decision thread shares with its [`DccService`].
pub(super) struct Shared {
    running: Arc<AtomicBool>,
    pub(super) context: Arc<RwLock<DccContext>>,
    registry: Arc<Mutex<AgentRegistry>>,
    budget_channel: Arc<RwLock<Option<BudgetChannel>>>,
    adaptation_modes: Arc<RwLock<HashMap<AgentId, AdaptationMode>>>,
    hints: Arc<RwLock<HashMap<AgentId, AgentHints>>>,
    pub(super) layout_recommendations: Arc<RwLock<HashMap<String, LayoutRecommendation>>>,
    pub(super) decision_recording: Arc<RwLock<Option<Instant>>>,
    recorded_trace: Arc<RwLock<DecisionTrace>>,
    seeded_costs: Arc<RwLock<Vec<ProfileRow>>>,
    learnt_costs: Arc<RwLock<Vec<ProfileRow>>>,
    decision_watch: DecisionWatch,
    replay: Arc<RwLock<Option<(DecisionTrace, usize)>>>,
    hardware_monitor: Arc<RwLock<Option<Arc<dyn HardwareMonitor>>>>,
}

/// The decision thread's state, carried from one tick to the next.
pub(super) struct DecisionLoop {
    pub(super) shared: Shared,
    pub(super) config: DccConfig,
    tick_duration: Duration,
    pub(super) store: MetricStore,
    heuristic_engine: HeuristicEngine,
    pub(super) arbitrator: GornaArbitrator,
    /// When the DCC started: the planner reads workload trends in it.
    pub(super) started: Instant,
    /// When the next epoch opens, and what waits to open it.
    pub(super) epochs: Epochs,
    /// Per-agent empirical cost models (`c·f(n)`), each read at the workload
    /// its own agent last reported.
    pub(super) cost_models: HashMap<AgentId, CostModel>,
    pub(super) workloads: HashMap<AgentId, f64>,
    /// Each agent's latest report, for the journal.
    pub(super) reports: HashMap<AgentId, Report>,
    /// The latest wave plan from the hot path; empty until the scheduler
    /// publishes one, and under serial execution.
    pub(super) latest_wave_plan: Vec<Vec<AgentId>>,
    /// Turns per-component access telemetry into a layout recommendation.
    pub(super) layout_advisor: LayoutAdvisor,
    /// Drives the global budget multiplier from the frames' critical paths,
    /// so the frame settles under its setpoint.
    governor: FrameGovernor,
    /// Whether the governor's last change still waits for an epoch to lend
    /// it: it does not decide again on frames that never saw it.
    change_unlent: bool,
    /// Multiplier in effect when budgets were last issued; one moved since
    /// opens an epoch.
    last_issued_multiplier: Option<f32>,
    /// The channel the allocation in force was last published on.
    ///
    /// One connected since is handed it, so an allocation decided before the
    /// scheduler connected is not lost.
    published_on: Option<BudgetChannel>,
    /// When the hardware monitor was last read: its sensors are slow to read,
    /// and heat and charge move slowly.
    hardware_read: Option<Instant>,
    /// The critical paths of the frames run since the last epoch, while
    /// decisions are recorded: the journal sums them up.
    pub(super) epoch_frames: Vec<Duration>,
    /// When the journal those frames belong to began: a journal begun since
    /// starts from no frame.
    pub(super) frames_of: Option<Instant>,
}

impl DecisionLoop {
    /// Builds the thread's state, seeding the cost book with what a profile
    /// learnt on an earlier run.
    fn new(shared: Shared, config: DccConfig) -> Self {
        let tick_duration = Duration::from_secs_f32(1.0 / config.tick_rate as f32);
        let store = MetricStore::new();
        let heuristic_engine = HeuristicEngine;
        let mut arbitrator =
            GornaArbitrator::new(Duration::from_millis(config.agent_lock_timeout_ms));
        *arbitrator.planner_mut() = crate::gorna::planning::Planner::new(config.planning);
        *arbitrator.cost_book_mut() = crate::gorna::cost_book::CostBook::new(config.cost_book);
        // What a profile learnt on an earlier run seeds the book, for the
        // agents registered now.
        let seeded = shared
            .seeded_costs
            .write()
            .map(|mut seeded| std::mem::take(&mut *seeded))
            .unwrap_or_default();
        if !seeded.is_empty() {
            let agents = shared
                .registry
                .lock()
                .map(|registry| registry.all_ids())
                .unwrap_or_default();
            arbitrator.cost_book_mut().seed(&seeded, &agents);
        }
        if let Ok(mut learnt) = shared.learnt_costs.write() {
            *learnt = arbitrator.cost_book().export();
        }
        let governor = FrameGovernor::new(config.frame_governor);
        Self {
            shared,
            config,
            tick_duration,
            store,
            heuristic_engine,
            arbitrator,
            started: Instant::now(),
            epochs: Epochs::default(),
            cost_models: HashMap::new(),
            workloads: HashMap::new(),
            reports: HashMap::new(),
            latest_wave_plan: Vec::new(),
            layout_advisor: LayoutAdvisor::default(),
            governor,
            change_unlent: false,
            last_issued_multiplier: None,
            published_on: None,
            hardware_read: None,
            epoch_frames: Vec::new(),
            frames_of: None,
        }
    }

    /// Ticks at the configured rate until the service stops.
    fn run(mut self, event_rx: &Receiver<TelemetryEvent>) {
        log::info!("DCC Service thread started.");

        while self.shared.running.load(Ordering::Relaxed) {
            let start_time = Instant::now();
            self.tick(event_rx);
            let elapsed = start_time.elapsed();
            if elapsed < self.tick_duration {
                thread::sleep(self.tick_duration - elapsed);
            }
        }
        log::info!("DCC Service thread stopped.");
    }

    fn tick(&mut self, event_rx: &Receiver<TelemetryEvent>) {
        self.read_hardware();
        let frames = self.ingest(event_rx);
        let (mut report, mut ctx_copy) = self.analyze();

        // The governor's setpoint is the heuristics' target, before the
        // forecast below tightens it: the forecast acts on what GORNA
        // shares, and counting it in the governor's target as well
        // would cut the same overrun twice.
        let setpoint_ms = report.suggested_latency_ms;
        self.tighten_to_forecast(&mut report);
        if report.death_spiral_detected {
            self.epochs.raise(Trigger::Safety);
        } else if report.needs_negotiation {
            self.epochs.raise(Trigger::Pressure);
        }

        let multiplier = self.govern(&frames, setpoint_ms, &mut report, &mut ctx_copy);

        // A multiplier moved either way re-arbitrates: pressure drives
        // downgrades, this drives recovery once the frames have room.
        if let Some(last) = self.last_issued_multiplier {
            if (multiplier - last).abs() > MULTIPLIER_EPSILON {
                self.epochs.raise(Trigger::FrameGovernor);
            }
        }

        for alert in &report.alerts {
            log::info!("DCC Analysis: {}", alert);
        }

        self.hand_in_force_to_new_channel();

        // GORNA. Only the agents of the families on take part; a
        // due trigger opens an epoch. The allocation is published
        // for the scheduler, which lends it at its next frame
        // start: this thread never lends.
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.shared.registry.lock_recovered();
            registry.iter_active(&ctx_copy.inactive).cloned().collect()
        };
        self.epochs.observe(&agents);
        // With no agent to lend it to, a change has nothing to wait
        // for.
        if agents.is_empty() && self.change_unlent {
            self.change_unlent = false;
            self.governor.lent();
        }

        // A replay publishes the recorded allocations, one a tick, in
        // order, until the trace is exhausted.
        let replayed = self
            .shared
            .replay
            .read()
            .ok()
            .and_then(|r| r.as_ref().and_then(|(t, c)| t.epochs.get(*c).cloned()));
        if let Some(record) = replayed {
            self.replay(record, &agents, multiplier);
        } else if let Some(trigger) = (self.epochs)
            .due(self.config.min_epoch_ticks)
            .filter(|_| !agents.is_empty())
        {
            self.open_epoch(trigger, &agents, &ctx_copy, &report, multiplier);
        }

        // What the book has learnt, for the host to keep across runs.
        if let Ok(mut learnt) = self.shared.learnt_costs.write() {
            *learnt = self.arbitrator.cost_book().export();
        }
    }

    /// Reads the hardware monitor into the context, at most once a
    /// [`HARDWARE_READ_PERIOD`].
    ///
    /// A sensor read can take milliseconds, and none of it may land in a
    /// frame. See the book: concepts/telemetry.md, "The hardware".
    fn read_hardware(&mut self) {
        if !(self.hardware_read).is_none_or(|at| at.elapsed() >= HARDWARE_READ_PERIOD) {
            return;
        }
        let monitor = self.shared.hardware_monitor.read_recovered().clone();
        if let Some(monitor) = monitor {
            // Read before the context is locked: the frame's
            // thread reads it every frame, and must never wait
            // on a sensor.
            monitor.refresh();
            let (thermal, battery, cpu_load) = (
                monitor.thermal_status(),
                monitor.battery_level(),
                monitor.cpu_load(),
            );
            let mut ctx = self.shared.context.write_recovered();
            ctx.hardware.thermal = thermal;
            ctx.hardware.battery = battery;
            ctx.hardware.cpu_load = cpu_load;
            self.hardware_read = Some(Instant::now());
        }
    }

    /// Runs the heuristics on the context; returns their report and a copy of
    /// the context they read.
    fn analyze(&mut self) -> (AnalysisReport, DccContext) {
        let mut ctx = self.shared.context.write_recovered();
        // The tracking allocator's figures give the memory pressure
        // the heuristics read.
        let mem_bytes = (self.store).get_average(&MetricId::new("memory", "current_bytes"));
        ctx.hardware.current_ram_bytes = (mem_bytes > 0.0).then_some(mem_bytes as u64);
        ctx.hardware.memory_budget_bytes = self.config.memory_budget_bytes;
        ctx.refresh_memory_pressure();
        let report = self.heuristic_engine.analyze(&ctx, &self.store);
        (report, ctx.clone())
    }

    /// Tightens the target GORNA shares when the cost models forecast a
    /// breach, so it downgrades before the frame overruns.
    ///
    /// See the book: concepts/arbiter.md, "The cost model and the frame governor".
    fn tighten_to_forecast(&mut self, report: &mut AnalysisReport) {
        if let Some(predicted_ms) = forecast_frame_ms(
            &self.cost_models,
            &self.workloads,
            &self.latest_wave_plan,
            self.arbitrator.background(),
        ) {
            if predicted_ms > report.suggested_latency_ms as f64 {
                let ratio =
                    (report.suggested_latency_ms as f64 / predicted_ms).clamp(0.5, 1.0) as f32;
                report.alerts.push(format!(
                    "Cost-model forecast {:.1}ms > budget {:.1}ms — tightening to {:.1}ms",
                    predicted_ms,
                    report.suggested_latency_ms,
                    report.suggested_latency_ms * ratio
                ));
                report.suggested_latency_ms *= ratio;
                self.epochs.raise(Trigger::ForecastBreach);
            }
        }
    }

    /// Feeds the tick's frames to the governor, lets it decide, and publishes
    /// the budget multiplier; returns the multiplier.
    ///
    /// The governor decides only on frames its last change reached, and never
    /// lowers with every agent at its floor; the safety ceiling caps what is
    /// published. See the book: concepts/arbiter.md, "The cost model and the
    /// frame governor".
    fn govern(
        &mut self,
        frames: &[Duration],
        setpoint_ms: f32,
        report: &mut AnalysisReport,
        ctx_copy: &mut DccContext,
    ) -> f32 {
        let setpoint = Duration::from_secs_f64(f64::from(setpoint_ms.max(0.0)) / 1_000.0);
        // The planner reads its margin from the samples the governor
        // keeps: no hitch, a load frame bounded.
        for frame in frames {
            if let Some(kept) = self.governor.observe(*frame, setpoint) {
                self.arbitrator.planner_mut().observe_frame(kept, setpoint);
            }
        }
        let frame_time_id = MetricId::new("frame", "critical_ms");
        // A replay lends what was recorded, whatever the multiplier:
        // the governor decides nothing on frames run under it.
        let replaying = self.shared.replay.read().ok().is_some_and(|r| r.is_some());
        if !self.change_unlent
            && !replaying
            && self.store.get_sample_count(&frame_time_id) >= FRAME_TIME_MIN_SAMPLES
        {
            let no_lever = (self.arbitrator)
                .in_force()
                .is_some_and(|allocation| !allocation.lever);
            if let Some(next) = self.governor.decide(setpoint, no_lever) {
                self.change_unlent = true;
                report.alerts.push(format!(
                    "Governor: budget multiplier → {next:.2} ({} frames this tick)",
                    frames.len()
                ));
            }
        }
        let multiplier = self.governor.multiplier().min(safety_ceiling(ctx_copy));
        ctx_copy.global_budget_multiplier = multiplier;
        if let Ok(mut ctx) = self.shared.context.write() {
            ctx.global_budget_multiplier = multiplier;
        }
        log::debug!(
            "DCC governor: multiplier={:.3} (setpoint={:.2}ms, {} frames this tick)",
            multiplier,
            setpoint_ms,
            frames.len()
        );
        multiplier
    }

    /// Hands the allocation in force to a channel connected since the last tick.
    fn hand_in_force_to_new_channel(&mut self) {
        if let Some(channel) = connected(&self.shared.budget_channel) {
            if !self.published_on.as_ref().is_some_and(|on| on.is(&channel)) {
                if let Some(allocation) = self.arbitrator.in_force() {
                    channel.publish(allocation.clone());
                }
                self.published_on = Some(channel);
            }
        }
    }

    /// Publishes a recorded allocation, and moves the replay's cursor past it.
    fn replay(&mut self, record: EpochRecord, agents: &[Arc<Mutex<dyn Agent>>], multiplier: f32) {
        self.arbitrator.reissue(&record);
        if let Some(view) = self.arbitrator.view(true) {
            self.shared.decision_watch.publish(view);
        }
        let allocation = record.allocation;
        if let Some(channel) = connected(&self.shared.budget_channel) {
            channel.publish_in_turn(allocation.clone());
        }
        let exhausted = (self.shared.replay.write().ok()).is_some_and(|mut r| match r.as_mut() {
            Some((trace, cursor)) => {
                *cursor += 1;
                *cursor >= trace.epochs.len()
            }
            None => false,
        });
        self.epochs.replayed(agents);
        self.epoch_frames.clear();
        self.epochs.set_in_force(allocation.epoch);
        // Once the trace runs out, a live epoch opens for the
        // engine as it is now, whatever the trace lent last.
        if exhausted {
            self.epochs.raise(Trigger::Refresh);
        }
        // A replayed allocation was decided before the
        // governor's change: it does not lend it.
        self.last_issued_multiplier = Some(multiplier);
    }

    /// Runs a GORNA round, journals it while decisions are recorded, and
    /// publishes its allocation.
    fn open_epoch(
        &mut self,
        trigger: Trigger,
        agents: &[Arc<Mutex<dyn Agent>>],
        ctx_copy: &DccContext,
        report: &AnalysisReport,
        multiplier: f32,
    ) {
        // Sync developer-control modes into the arbitrator (the
        // host may have changed them) and snapshot the hints.
        let policies = (self.shared.adaptation_modes.read())
            .map(|m| m.clone())
            .unwrap_or_default();
        for (id, mode) in &policies {
            self.arbitrator.set_adaptation_mode(*id, *mode);
        }
        let tick_hints: HashMap<AgentId, AgentHints> = self
            .shared
            .hints
            .read()
            .map(|h| h.clone())
            .unwrap_or_default();
        self.arbitrator.set_wave_plan(&self.latest_wave_plan);
        // Each cost model's class reads its agent's workload
        // trend into a growth over the horizon.
        for (id, model) in &self.cost_models {
            if let Some((class, _)) = model.best_fit() {
                self.arbitrator.planner_mut().observe_class(*id, class);
            }
        }
        // The target the frames since the last epoch ran under,
        // and the envelopes the round opens with.
        let ran_under = self.arbitrator.target();
        let in_force = (self.arbitrator.in_force())
            .map(|allocation| allocation.envelopes.clone())
            .unwrap_or_default();
        let allocation = (self.arbitrator).allocate(ctx_copy, report, trigger, agents, &tick_hints);
        if let Some(view) = self.arbitrator.view(false) {
            self.shared.decision_watch.publish(view);
        }
        self.epochs.opened(agents);
        self.epochs.set_in_force(allocation.epoch);
        self.last_issued_multiplier = Some(multiplier);
        if self.change_unlent {
            self.change_unlent = false;
            self.governor.lent();
        }
        let ran = std::mem::take(&mut self.epoch_frames);
        if let Some(since) = recording_since(&self.shared.decision_recording) {
            let journal = Journal {
                since,
                ran,
                ran_under,
                in_force,
                policies: &policies,
                hints: &tick_hints,
            };
            self.record_epoch(&allocation, report, journal);
        }
        if let Some(channel) = connected(&self.shared.budget_channel) {
            channel.publish(allocation);
        }
    }

    /// Appends the round just run to the recorded trace.
    fn record_epoch(&mut self, allocation: &Allocation, report: &AnalysisReport, journal: Journal) {
        let Journal {
            since,
            ran,
            ran_under,
            in_force,
            policies,
            hints,
        } = journal;
        // A journal begun while the round was open holds
        // none of the frames buffered for the one before.
        let ran = if self.frames_of == Some(since) {
            ran
        } else {
            Vec::new()
        };
        self.frames_of = Some(since);
        // Before the first round, the frames ran under no
        // target yet: the first one stands for it.
        let ran_under = if ran_under.is_zero() {
            self.arbitrator.target()
        } else {
            ran_under
        };
        let arbitrator = &self.arbitrator;
        if let Ok(mut trace) = self.shared.recorded_trace.write() {
            trace.epochs.push(EpochRecord {
                allocation: allocation.clone(),
                target: arbitrator.target(),
                offers: arbitrator.offers().to_vec(),
                reports: by_agent(&self.reports)
                    .into_iter()
                    .map(|(_, run)| run)
                    .collect(),
                waves: self.latest_wave_plan.clone(),
                policies: by_agent(policies),
                hints: by_agent(hints),
                reasons: report.alerts.clone(),
                why: arbitrator.why().clone(),
                importance: arbitrator.importance().to_vec(),
                corrections: arbitrator.corrections().to_vec(),
                memory: arbitrator.memory(),
                background: arbitrator.background().to_vec(),
                at: since.elapsed(),
                frames: summary(&ran, ran_under),
                in_force,
                planning: arbitrator.planning().clone(),
                planned: arbitrator.planned().to_vec(),
            });
        }
    }
}

/// What a round's journal entry reads beyond the arbitrator.
struct Journal<'a> {
    /// When the journal began.
    since: Instant,
    /// The critical paths of the frames run since the last epoch.
    ran: Vec<Duration>,
    /// The target those frames ran under.
    ran_under: Duration,
    /// The envelopes the round opened with.
    in_force: Vec<(AgentId, Envelope)>,
    policies: &'a HashMap<AgentId, AdaptationMode>,
    hints: &'a HashMap<AgentId, AgentHints>,
}

/// Returns when the decisions started being recorded, if they are.
pub(super) fn recording_since(recording: &std::sync::RwLock<Option<Instant>>) -> Option<Instant> {
    recording.read().ok().and_then(|since| *since)
}

/// Sums up `frames`, counting those over `target`.
fn summary(frames: &[Duration], target: Duration) -> FrameSummary {
    let count = frames.len() as u32;
    FrameSummary {
        count,
        over_target: frames.iter().filter(|frame| **frame > target).count() as u32,
        worst: frames.iter().copied().max().unwrap_or_default(),
        mean: if count == 0 {
            Duration::ZERO
        } else {
            frames.iter().sum::<Duration>() / count
        },
    }
}

/// Returns `map`'s entries in the agents' order, so a journal reads the same from one
/// run to the next.
fn by_agent<T: Clone>(map: &HashMap<AgentId, T>) -> Vec<(AgentId, T)> {
    let mut entries: Vec<(AgentId, T)> =
        map.iter().map(|(id, value)| (*id, value.clone())).collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}
