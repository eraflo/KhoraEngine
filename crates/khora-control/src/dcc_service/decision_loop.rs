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
use super::{
    connected, DccService, COST_MODEL_CAPACITY, FRAME_TIME_MIN_SAMPLES, MULTIPLIER_EPSILON,
};
use crate::cost_model::CostModel;
use crate::dcc_context::safety_ceiling;
use crate::governor::FrameGovernor;
use crate::metrics::MetricStore;
use crossbeam_channel::Receiver;
use khora_core::agent::Agent;
use khora_core::telemetry::monitoring::MonitoredResourceType;
use khora_core::telemetry::{MetricId, TelemetryEvent};
use khora_data::ecs::layout::LayoutAdvisor;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use khora_core::agent::gorna::protocol::{Report, Trigger};
use khora_core::agent::gorna::{AgentHints, AgentId, EpochRecord, FrameSummary};
use std::collections::HashMap;

/// The share of the platform's video-memory budget kept in reserve.
///
/// The budget moves with the other programs on the GPU, and running at its
/// edge invites eviction. See the book: concepts/telemetry.md, "Video memory".
const VRAM_HEADROOM: f64 = 0.1;

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
        let running = Arc::clone(&self.running);
        let context = Arc::clone(&self.context);
        let registry = Arc::clone(&self.registry);
        let budget_channel = Arc::clone(&self.budget_channel);
        let adaptation_modes = Arc::clone(&self.adaptation_modes);
        let hints = Arc::clone(&self.hints);
        let layout_recommendations = Arc::clone(&self.layout_recommendations);
        let decision_recording = Arc::clone(&self.decision_recording);
        let recorded_trace = Arc::clone(&self.recorded_trace);
        let seeded_costs = Arc::clone(&self.seeded_costs);
        let learnt_costs = Arc::clone(&self.learnt_costs);
        let decision_watch = self.decision_watch.clone();
        let replay = Arc::clone(&self.replay);
        let hardware_monitor = Arc::clone(&self.hardware_monitor);
        let tick_duration = Duration::from_secs_f32(1.0 / self.config.tick_rate as f32);
        let agent_lock_timeout = Duration::from_millis(self.config.agent_lock_timeout_ms);
        let memory_budget_bytes = self.config.memory_budget_bytes;
        let vram_budget_bytes = self.config.vram_budget_bytes;
        let governor_cfg = self.config.frame_governor;
        let planning_cfg = self.config.planning;
        let cost_book_cfg = self.config.cost_book;

        let min_epoch_ticks = self.config.min_epoch_ticks;
        let overruns_in_a_row = self.config.overruns_in_a_row;
        let handle = thread::spawn(move || {
            let mut store = MetricStore::new();
            let heuristic_engine = HeuristicEngine;
            let mut arbitrator = GornaArbitrator::new(agent_lock_timeout);
            *arbitrator.planner_mut() = crate::gorna::planning::Planner::new(planning_cfg);
            *arbitrator.cost_book_mut() = crate::gorna::cost_book::CostBook::new(cost_book_cfg);
            // What a profile learnt on an earlier run seeds the book, for the
            // agents registered now.
            let seeded = seeded_costs
                .write()
                .map(|mut seeded| std::mem::take(&mut *seeded))
                .unwrap_or_default();
            if !seeded.is_empty() {
                let agents = registry
                    .lock()
                    .map(|registry| registry.all_ids())
                    .unwrap_or_default();
                arbitrator.cost_book_mut().seed(&seeded, &agents);
            }
            if let Ok(mut learnt) = learnt_costs.write() {
                *learnt = arbitrator.cost_book().export();
            }
            // When the DCC started: the planner reads workload trends in it.
            let started = Instant::now();
            // When the next epoch opens, and what waits to open it.
            let mut epochs = Epochs::default();
            // Per-agent empirical cost models (`c·f(n)`), each read at the
            // workload its own agent last reported.
            let mut cost_models: HashMap<AgentId, CostModel> = HashMap::new();
            let mut workloads: HashMap<AgentId, f64> = HashMap::new();
            // Each agent's latest report, for the journal.
            let mut reports: HashMap<AgentId, Report> = HashMap::new();
            // The latest wave plan from the hot path; empty until the scheduler
            // publishes one, and under serial execution.
            let mut latest_wave_plan: Vec<Vec<AgentId>> = Vec::new();
            // Turns per-component access telemetry into a layout recommendation.
            let layout_advisor = LayoutAdvisor::default();
            // The frame governor: drives the global budget multiplier from the
            // frames' critical paths, so the frame settles under its setpoint.
            let mut governor = FrameGovernor::new(governor_cfg);
            // Whether the governor's last change still waits for an epoch to
            // lend it: it does not decide again on frames that never saw it.
            let mut change_unlent = false;
            // Multiplier in effect when budgets were last issued: one that has
            // moved since — a governor decision, the safety ceiling — opens an
            // epoch.
            let mut last_issued_multiplier: Option<f32> = None;
            // The channel the allocation in force was last published on: one
            // connected since is handed it, so an allocation decided before
            // the scheduler connected is not lost.
            let mut published_on: Option<crate::budget_channel::BudgetChannel> = None;
            // When the hardware monitor was last read: its sensors are slow to
            // read, and heat and charge move slowly.
            let mut hardware_read: Option<Instant> = None;
            // The critical paths of the frames run since the last epoch,
            // while decisions are recorded: the journal sums them up.
            let mut epoch_frames: Vec<Duration> = Vec::new();
            // When the journal those frames belong to began: a journal begun
            // since starts from no frame.
            let mut frames_of: Option<Instant> = None;

            log::info!("DCC Service thread started.");

            while running.load(Ordering::Relaxed) {
                let start_time = Instant::now();

                // A sensor read can take milliseconds, and none of it may land
                // in a frame. See the book: concepts/telemetry.md, "The hardware".
                if hardware_read.is_none_or(|at| at.elapsed() >= HARDWARE_READ_PERIOD) {
                    let monitor = hardware_monitor
                        .read()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
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
                        let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                        ctx.hardware.thermal = thermal;
                        ctx.hardware.battery = battery;
                        ctx.hardware.cpu_load = cpu_load;
                        hardware_read = Some(Instant::now());
                    }
                }

                // The frames reported during this tick, in order, are what the
                // governor observes once the tick's setpoint is known.
                let mut frames: Vec<Duration> = Vec::new();
                let journal = recording_since(&decision_recording);
                if journal != frames_of {
                    epoch_frames.clear();
                    frames_of = journal;
                }
                while let Ok(event) = event_rx.try_recv() {
                    match event {
                        TelemetryEvent::MetricUpdate { id, value } => {
                            if let Some(v) = value.as_f64() {
                                store.push(id, v as f32);
                            }
                        }
                        TelemetryEvent::ResourceReport { resource, usage } => {
                            // A report with neither a budget nor a capacity
                            // says nothing of what is left: the figure the
                            // DCC knew stays.
                            if resource == MonitoredResourceType::Vram {
                                store.push(
                                    MetricId::new("vram", "current_bytes"),
                                    usage.current_bytes as f32,
                                );
                                // A developer's budget is taken whole; the
                                // platform's keeps a tenth in reserve.
                                let ceiling = vram_budget_bytes.or(usage.total_capacity_bytes.map(
                                    |capacity| (capacity as f64 * (1.0 - VRAM_HEADROOM)) as u64,
                                ));
                                if let Some(ceiling) = ceiling {
                                    let mut ctx =
                                        context.write().unwrap_or_else(|e| e.into_inner());
                                    ctx.hardware.available_vram =
                                        Some(ceiling.saturating_sub(usage.current_bytes));
                                }
                            }
                        }
                        TelemetryEvent::HardwareReport(report) => {
                            let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                            ctx.hardware.thermal = report.thermal;
                            ctx.hardware.battery = report.battery;
                            ctx.hardware.cpu_load = report.cpu_load;
                            ctx.hardware.gpu_load = report.gpu_load.unwrap_or(0.0);
                            // A hardware report carries no VRAM figure: the
                            // one the DCC knows stays.

                            log::debug!(
                                "DCC Hardware: Thermal={:?}, CPU={:.2}, GPU={:?}",
                                ctx.hardware.thermal,
                                ctx.hardware.cpu_load,
                                ctx.hardware.gpu_load
                            );
                        }
                        TelemetryEvent::ActivationChanged(inactive) => {
                            let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                            log::debug!("DCC families off: {:?} → {:?}", ctx.inactive, inactive);
                            if ctx.inactive != inactive {
                                // The agents asked change with the families on:
                                // raised here rather than left to the next
                                // tick's comparison, which a switch made before
                                // the agents were first seen would not see.
                                epochs.raise(Trigger::AgentsChanged);
                                // Runs with other families on say nothing of
                                // these.
                                reports.clear();
                            }
                            ctx.inactive = inactive;
                        }
                        TelemetryEvent::AgentReport(run) => {
                            let id = run.agent;
                            let time = &run.measured;
                            let time_ms = time
                                .cpu
                                .max(time.gpu)
                                .max(time.npu)
                                .max(time.other)
                                .as_secs_f64()
                                * 1_000.0;
                            if let Some(n) = run.workload {
                                workloads.insert(id, n);
                                cost_models
                                    .entry(id)
                                    .or_insert_with(|| CostModel::new(COST_MODEL_CAPACITY))
                                    .record(n, time_ms);
                            }
                            store.push(
                                khora_core::telemetry::MetricId::new(
                                    "agent",
                                    format!("{id}_time_ms"),
                                ),
                                time_ms as f32,
                            );
                            epochs.report(&run, overruns_in_a_row);
                            arbitrator
                                .planner_mut()
                                .observe_report(&run, started.elapsed());
                            // Each run teaches the book its row's measured
                            // time against the time it quoted.
                            arbitrator.cost_book_mut().observe_report(&run);
                            reports.insert(id, run);
                        }
                        TelemetryEvent::FrameReport(frame) => {
                            let busy = &frame.busy;
                            for (device, time) in [
                                ("cpu_ms", busy.cpu),
                                ("gpu_ms", busy.gpu),
                                ("npu_ms", busy.npu),
                                ("other_ms", busy.other),
                            ] {
                                if let Some(time) = time {
                                    store.push(
                                        MetricId::new("frame", device),
                                        time.as_secs_f32() * 1_000.0,
                                    );
                                }
                            }
                            if let Some(critical) = frame.critical() {
                                let ms = critical.as_secs_f32() * 1_000.0;
                                store.push(MetricId::new("frame", "critical_ms"), ms);
                                frames.push(critical);
                                if journal.is_some() {
                                    epoch_frames.push(critical);
                                }
                            }
                        }
                        TelemetryEvent::WavePlan { waves } => {
                            // Republished each frame: the latest wins.
                            latest_wave_plan = waves;
                        }
                        TelemetryEvent::ComponentAccess {
                            type_name,
                            size_bytes,
                            query_count,
                            rows_scanned,
                        } => {
                            // Advice only: the DCC never repacks, Data owns its
                            // layout.
                            let recommendation =
                                layout_advisor.recommend(size_bytes, query_count, rows_scanned);
                            if let Ok(mut recs) = layout_recommendations.write() {
                                recs.insert(type_name.clone(), recommendation);
                            }
                            store.push(
                                khora_core::telemetry::MetricId::new("ecs_access", type_name),
                                rows_scanned as f32,
                            );
                        }
                    }
                }

                let (mut report, mut ctx_copy) = {
                    let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                    // The tracking allocator's figures give the memory pressure
                    // the heuristics read.
                    let mem_bytes = store.get_average(&MetricId::new("memory", "current_bytes"));
                    ctx.hardware.current_ram_bytes = (mem_bytes > 0.0).then_some(mem_bytes as u64);
                    ctx.hardware.memory_budget_bytes = memory_budget_bytes;
                    ctx.refresh_memory_pressure();
                    let report = heuristic_engine.analyze(&ctx, &store);
                    (report, ctx.clone())
                };

                // The governor's setpoint is the heuristics' target, before the
                // forecast below tightens it: the forecast acts on what GORNA
                // shares, and counting it in the governor's target as well
                // would cut the same overrun twice.
                let setpoint_ms = report.suggested_latency_ms;

                // A forecast breach tightens the target GORNA shares, so it
                // downgrades before the frame overruns. See the book:
                // concepts/gorna.md, "The cost model and the frame governor".
                if let Some(predicted_ms) = forecast_frame_ms(
                    &cost_models,
                    &workloads,
                    &latest_wave_plan,
                    arbitrator.background(),
                ) {
                    if predicted_ms > report.suggested_latency_ms as f64 {
                        let ratio = (report.suggested_latency_ms as f64 / predicted_ms)
                            .clamp(0.5, 1.0) as f32;
                        report.alerts.push(format!(
                            "Cost-model forecast {:.1}ms > budget {:.1}ms — tightening to {:.1}ms",
                            predicted_ms,
                            report.suggested_latency_ms,
                            report.suggested_latency_ms * ratio
                        ));
                        report.suggested_latency_ms *= ratio;
                        epochs.raise(Trigger::ForecastBreach);
                    }
                }
                if report.death_spiral_detected {
                    epochs.raise(Trigger::Safety);
                } else if report.needs_negotiation {
                    epochs.raise(Trigger::Pressure);
                }

                // The governor decides only on frames its last change reached,
                // and never lowers with every agent at its floor; the safety
                // ceiling caps what is published. See the book:
                // concepts/gorna.md, "The cost model and the frame governor".
                let setpoint = Duration::from_secs_f64(f64::from(setpoint_ms.max(0.0)) / 1_000.0);
                // The planner reads its margin from the samples the governor
                // keeps: no hitch, a load frame bounded.
                for frame in &frames {
                    if let Some(kept) = governor.observe(*frame, setpoint) {
                        arbitrator.planner_mut().observe_frame(kept, setpoint);
                    }
                }
                let frame_time_id = MetricId::new("frame", "critical_ms");
                // A replay lends what was recorded, whatever the multiplier:
                // the governor decides nothing on frames run under it.
                let replaying = replay.read().ok().is_some_and(|r| r.is_some());
                if !change_unlent
                    && !replaying
                    && store.get_sample_count(&frame_time_id) >= FRAME_TIME_MIN_SAMPLES
                {
                    let no_lever = arbitrator
                        .in_force()
                        .is_some_and(|allocation| !allocation.lever);
                    if let Some(next) = governor.decide(setpoint, no_lever) {
                        change_unlent = true;
                        report.alerts.push(format!(
                            "Governor: budget multiplier → {next:.2} ({} frames this tick)",
                            frames.len()
                        ));
                    }
                }
                let multiplier = governor.multiplier().min(safety_ceiling(&ctx_copy));
                ctx_copy.global_budget_multiplier = multiplier;
                if let Ok(mut ctx) = context.write() {
                    ctx.global_budget_multiplier = multiplier;
                }
                log::debug!(
                    "DCC governor: multiplier={:.3} (setpoint={:.2}ms, {} frames this tick)",
                    multiplier,
                    setpoint_ms,
                    frames.len()
                );

                // A multiplier moved either way re-arbitrates: pressure drives
                // downgrades, this drives recovery once the frames have room.
                if let Some(last) = last_issued_multiplier {
                    if (multiplier - last).abs() > MULTIPLIER_EPSILON {
                        epochs.raise(Trigger::FrameGovernor);
                    }
                }

                for alert in &report.alerts {
                    log::info!("DCC Analysis: {}", alert);
                }

                // A channel connected since the last tick is handed the
                // allocation in force.
                if let Some(channel) = connected(&budget_channel) {
                    if !published_on.as_ref().is_some_and(|on| on.is(&channel)) {
                        if let Some(allocation) = arbitrator.in_force() {
                            channel.publish(allocation.clone());
                        }
                        published_on = Some(channel);
                    }
                }

                // GORNA. Only the agents of the families on take part; a
                // due trigger opens an epoch. The allocation is published
                // for the scheduler, which lends it at its next frame
                // start: this thread never lends.
                let agents: Vec<Arc<std::sync::Mutex<dyn Agent>>> = {
                    let registry = registry.lock().unwrap_or_else(|e| e.into_inner());
                    registry.iter_active(&ctx_copy.inactive).cloned().collect()
                };
                epochs.observe(&agents);
                // With no agent to lend it to, a change has nothing to wait
                // for.
                if agents.is_empty() && change_unlent {
                    change_unlent = false;
                    governor.lent();
                }

                // A replay publishes the recorded allocations, one a tick, in
                // order, until the trace is exhausted.
                let replayed = replay
                    .read()
                    .ok()
                    .and_then(|r| r.as_ref().and_then(|(t, c)| t.epochs.get(*c).cloned()));
                if let Some(record) = replayed {
                    arbitrator.reissue(&record);
                    if let Some(view) = arbitrator.view(true) {
                        decision_watch.publish(view);
                    }
                    let allocation = record.allocation;
                    if let Some(channel) = connected(&budget_channel) {
                        channel.publish_in_turn(allocation.clone());
                    }
                    let exhausted = replay.write().ok().is_some_and(|mut r| match r.as_mut() {
                        Some((trace, cursor)) => {
                            *cursor += 1;
                            *cursor >= trace.epochs.len()
                        }
                        None => false,
                    });
                    epochs.replayed(&agents);
                    epoch_frames.clear();
                    epochs.set_in_force(allocation.epoch);
                    // Once the trace runs out, a live epoch opens for the
                    // engine as it is now, whatever the trace lent last.
                    if exhausted {
                        epochs.raise(Trigger::Refresh);
                    }
                    // A replayed allocation was decided before the
                    // governor's change: it does not lend it.
                    last_issued_multiplier = Some(multiplier);
                } else if let Some(trigger) =
                    epochs.due(min_epoch_ticks).filter(|_| !agents.is_empty())
                {
                    // Sync developer-control modes into the arbitrator (the
                    // host may have changed them) and snapshot the hints.
                    let policies = adaptation_modes
                        .read()
                        .map(|m| m.clone())
                        .unwrap_or_default();
                    for (id, mode) in &policies {
                        arbitrator.set_adaptation_mode(*id, *mode);
                    }
                    let tick_hints: HashMap<AgentId, AgentHints> =
                        hints.read().map(|h| h.clone()).unwrap_or_default();
                    arbitrator.set_wave_plan(&latest_wave_plan);
                    // Each cost model's class reads its agent's workload
                    // trend into a growth over the horizon.
                    for (id, model) in &cost_models {
                        if let Some((class, _)) = model.best_fit() {
                            arbitrator.planner_mut().observe_class(*id, class);
                        }
                    }
                    // The target the frames since the last epoch ran under,
                    // and the envelopes the round opens with.
                    let ran_under = arbitrator.target();
                    let in_force = (arbitrator.in_force())
                        .map(|allocation| allocation.envelopes.clone())
                        .unwrap_or_default();
                    let allocation =
                        arbitrator.allocate(&ctx_copy, &report, trigger, &agents, &tick_hints);
                    if let Some(view) = arbitrator.view(false) {
                        decision_watch.publish(view);
                    }
                    epochs.opened(&agents);
                    epochs.set_in_force(allocation.epoch);
                    last_issued_multiplier = Some(multiplier);
                    if change_unlent {
                        change_unlent = false;
                        governor.lent();
                    }
                    let ran = std::mem::take(&mut epoch_frames);
                    if let Some(since) = recording_since(&decision_recording) {
                        // A journal begun while the round was open holds
                        // none of the frames buffered for the one before.
                        let ran = if frames_of == Some(since) {
                            ran
                        } else {
                            Vec::new()
                        };
                        frames_of = Some(since);
                        // Before the first round, the frames ran under no
                        // target yet: the first one stands for it.
                        let ran_under = if ran_under.is_zero() {
                            arbitrator.target()
                        } else {
                            ran_under
                        };
                        if let Ok(mut trace) = recorded_trace.write() {
                            trace.epochs.push(EpochRecord {
                                allocation: allocation.clone(),
                                target: arbitrator.target(),
                                offers: arbitrator.offers().to_vec(),
                                reports: by_agent(&reports)
                                    .into_iter()
                                    .map(|(_, run)| run)
                                    .collect(),
                                waves: latest_wave_plan.clone(),
                                policies: by_agent(&policies),
                                hints: by_agent(&tick_hints),
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
                    if let Some(channel) = connected(&budget_channel) {
                        channel.publish(allocation);
                    }
                }

                // What the book has learnt, for the host to keep across runs.
                if let Ok(mut learnt) = learnt_costs.write() {
                    *learnt = arbitrator.cost_book().export();
                }

                let elapsed = start_time.elapsed();
                if elapsed < tick_duration {
                    thread::sleep(tick_duration - elapsed);
                }
            }
            log::info!("DCC Service thread stopped.");
        });

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

/// When the decisions started being recorded, if they are.
fn recording_since(recording: &std::sync::RwLock<Option<Instant>>) -> Option<Instant> {
    recording.read().ok().and_then(|since| *since)
}

/// `frames` summed up, those over `target` counted.
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

/// `map`'s entries in the agents' order, so a journal reads the same from one
/// run to the next.
fn by_agent<T: Clone>(map: &HashMap<AgentId, T>) -> Vec<(AgentId, T)> {
    let mut entries: Vec<(AgentId, T)> =
        map.iter().map(|(id, value)| (*id, value.clone())).collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}
