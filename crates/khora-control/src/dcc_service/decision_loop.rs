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
    connected, DccService, COST_MODEL_CAPACITY, FRAME_TIME_MIN_SAMPLES, PID_RENEGOTIATE_DELTA,
};
use crate::cost_model::CostModel;
use crate::dcc_context::safety_ceiling;
use crate::metrics::MetricStore;
use crate::pid::PidController;
use crossbeam_channel::Receiver;
use khora_core::agent::Agent;
use khora_core::telemetry::{MetricId, TelemetryEvent};
use khora_data::ecs::layout::LayoutAdvisor;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use khora_core::agent::gorna::protocol::{Report, Trigger};
use khora_core::agent::gorna::{AgentHints, AgentId, EpochRecord};
use std::collections::HashMap;

impl DccService {
    /// Starts the DCC background thread.
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
        let replay = Arc::clone(&self.replay);
        let tick_duration = Duration::from_secs_f32(1.0 / self.config.tick_rate as f32);
        let agent_lock_timeout = Duration::from_millis(self.config.agent_lock_timeout_ms);
        let memory_budget_bytes = self.config.memory_budget_bytes;
        let frame_pid_cfg = self.config.frame_pid;

        let min_epoch_ticks = self.config.min_epoch_ticks;
        let overruns_in_a_row = self.config.overruns_in_a_row;
        let handle = thread::spawn(move || {
            let mut store = MetricStore::new();
            let heuristic_engine = HeuristicEngine;
            let mut arbitrator = GornaArbitrator::new(agent_lock_timeout);
            // When the next epoch opens, and what waits to open it.
            let mut epochs = Epochs::default();
            // Per-agent empirical cost models (`c·f(n)`), fed from the reports
            // the scheduler publishes and used to forecast budget breaches
            // before they happen, each read at the workload its own agent last
            // reported.
            let mut cost_models: HashMap<AgentId, CostModel> = HashMap::new();
            let mut workloads: HashMap<AgentId, f64> = HashMap::new();
            // Each agent's latest report, for the journal.
            let mut reports: HashMap<AgentId, Report> = HashMap::new();
            // Latest wave plan from the hot path: how agents are grouped for
            // concurrent execution. Empty until the scheduler publishes one (and
            // stays empty under serial execution), in which case cost fitting
            // falls back to summing per-agent costs.
            let mut latest_wave_plan: Vec<Vec<AgentId>> = Vec::new();
            // Read-only layout advisor: turns per-component access telemetry into
            // a recommendation for the glass-box. Stateless (a tuned heuristic).
            let layout_advisor = LayoutAdvisor::default();
            // Frame-time PID: regulates the global budget multiplier so measured
            // frame time tracks the heuristic-suggested latency. Persists across
            // ticks; `last_tick` gives the real `dt` between updates.
            let mut frame_pid = PidController::new(frame_pid_cfg);
            let mut last_tick: Option<Instant> = None;
            // Multiplier in effect when budgets were last issued — drift beyond
            // PID_RENEGOTIATE_DELTA re-arbitrates (closes the recovery path).
            let mut last_issued_multiplier: Option<f32> = None;
            // The channel the allocation in force was last published on: one
            // connected since is handed it, so an allocation decided before
            // the scheduler connected is not lost.
            let mut published_on: Option<crate::budget_channel::BudgetChannel> = None;

            log::info!("DCC Service thread started.");

            while running.load(Ordering::Relaxed) {
                let start_time = Instant::now();

                // 1. Ingest all pending events
                while let Ok(event) = event_rx.try_recv() {
                    match event {
                        TelemetryEvent::MetricUpdate { id, value } => {
                            if let Some(v) = value.as_f64() {
                                store.push(id, v as f32);
                            }
                        }
                        TelemetryEvent::ResourceReport(_) => {}
                        TelemetryEvent::HardwareReport(report) => {
                            let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                            ctx.hardware.thermal = report.thermal;
                            ctx.hardware.battery = report.battery;
                            ctx.hardware.cpu_load = report.cpu_load;
                            ctx.hardware.gpu_load = report.gpu_load.unwrap_or(0.0);
                            // A hardware report carries no VRAM figure: the
                            // one the DCC knows stays.

                            if let Some(gpu_timings) = report.gpu_timings {
                                if let Some(frame_time_us) = gpu_timings.frame_total_duration_us() {
                                    store.push(
                                        khora_core::telemetry::MetricId::new(
                                            "renderer",
                                            "frame_time",
                                        ),
                                        frame_time_us as f32 / 1000.0,
                                    );
                                }
                            }

                            log::debug!(
                                "DCC Hardware: Thermal={:?}, CPU={:.2}, GPU={:?}",
                                ctx.hardware.thermal,
                                ctx.hardware.cpu_load,
                                ctx.hardware.gpu_load
                            );
                        }
                        TelemetryEvent::ModeChange(new_mode) => {
                            let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                            log::debug!("DCC Mode: {:?} → {:?}", ctx.mode, new_mode);
                            if ctx.mode != new_mode {
                                epochs.raise(Trigger::ModeChanged);
                                // Runs of another mode say nothing of this one.
                                reports.clear();
                            }
                            ctx.mode = new_mode;
                        }
                        TelemetryEvent::GpuReport(report) => {
                            if let Some(frame_time_us) = report.frame_total_duration_us() {
                                store.push(
                                    khora_core::telemetry::MetricId::new(
                                        "renderer",
                                        "gpu_frame_time",
                                    ),
                                    frame_time_us as f32 / 1000.0,
                                );
                            }
                            store.push(
                                khora_core::telemetry::MetricId::new("renderer", "draw_calls"),
                                report.draw_calls as f32,
                            );
                            store.push(
                                khora_core::telemetry::MetricId::new(
                                    "renderer",
                                    "triangles_rendered",
                                ),
                                report.triangles_rendered as f32,
                            );
                        }
                        TelemetryEvent::AgentReport(run) => {
                            // A run's report feeds the agent's empirical cost
                            // model at the run's workload, surfaces the run's
                            // time as a glass-box metric, may raise a trigger,
                            // and is kept for the journal.
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
                            reports.insert(id, run);
                        }
                        TelemetryEvent::WavePlan { waves } => {
                            // The hot path republishes this each frame; keep the
                            // latest so arbitration costs concurrent waves by
                            // their critical path.
                            latest_wave_plan = waves;
                        }
                        TelemetryEvent::ComponentAccess {
                            type_name,
                            size_bytes,
                            query_count,
                            rows_scanned,
                        } => {
                            // Advise (read-only) which layout this component would
                            // benefit from, and surface rows-scanned for the
                            // glass-box. The DCC never repacks — Data owns layout.
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

                // 2. Perform Analysis & Arbitration
                let (mut report, mut ctx_copy) = {
                    let mut ctx = context.write().unwrap_or_else(|e| e.into_inner());
                    // Fold the latest tracking-allocator telemetry into the
                    // context so memory pressure influences the budget alongside
                    // thermal/battery (the allocator's data drives a decision).
                    let mem_bytes = store.get_average(&MetricId::new("memory", "current_bytes"));
                    ctx.hardware.current_ram_bytes = (mem_bytes > 0.0).then_some(mem_bytes as u64);
                    ctx.hardware.memory_budget_bytes = memory_budget_bytes;
                    ctx.refresh_memory_pressure();
                    let report = heuristic_engine.analyze(&ctx, &store);
                    (report, ctx.clone())
                };

                // 2b. Anticipatory budgeting: the empirical per-agent cost models
                //     forecast the combined frame cost at the current workload. If
                //     that exceeds the budget, negotiate now and tighten the target
                //     so GORNA downgrades *before* the frame actually overruns —
                //     turning the reactive loop predictive (model proposes,
                //     measurement disposes).
                if let Some(predicted_ms) =
                    forecast_frame_ms(&cost_models, &workloads, &latest_wave_plan)
                {
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

                // 2b-bis. Frame-time PID: regulate the global budget multiplier so
                //     the measured frame time tracks the (now-finalized) setpoint
                //     `report.suggested_latency_ms`. The setpoint already carries
                //     the thermal/battery/phase modulation; the loop converges the
                //     multiplier smoothly instead of stepping it. A hard safety
                //     ceiling (Critical thermal/battery, near-budget memory) caps
                //     it immediately, independent of loop convergence.
                let dt = last_tick
                    .map(|t| start_time.duration_since(t).as_secs_f32())
                    .unwrap_or_else(|| tick_duration.as_secs_f32());
                last_tick = Some(start_time);

                let frame_time_id = MetricId::new("renderer", "frame_time");
                let measured = store.get_average(&frame_time_id);
                let mut multiplier = if store.get_sample_count(&frame_time_id)
                    >= FRAME_TIME_MIN_SAMPLES
                    && measured > 0.0
                {
                    frame_pid.update(report.suggested_latency_ms, measured, dt)
                } else {
                    frame_pid.output()
                };
                multiplier = multiplier.min(safety_ceiling(&ctx_copy));
                ctx_copy.global_budget_multiplier = multiplier;
                if let Ok(mut ctx) = context.write() {
                    ctx.global_budget_multiplier = multiplier;
                }
                log::debug!(
                    "DCC PID: multiplier={:.3} (setpoint={:.2}ms, measured={:.2}ms, dt={:.3}s)",
                    multiplier,
                    report.suggested_latency_ms,
                    measured,
                    dt
                );

                // 2b-ter. Re-arbitrate when the PID has moved the effective budget
                //     significantly since the last issuance — in both directions.
                //     Pressure heuristics drive downgrades; this drives recovery:
                //     once measured frame time settles under the setpoint, the
                //     multiplier climbs back and budgets are re-issued so agents
                //     can upgrade instead of staying degraded forever.
                if let Some(last) = last_issued_multiplier {
                    if (multiplier - last).abs() > PID_RENEGOTIATE_DELTA {
                        epochs.raise(Trigger::PidDrift);
                        report.alerts.push(format!(
                            "PID: budget multiplier {last:.2} → {multiplier:.2} since the last epoch — re-allocating",
                        ));
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

                // 3. GORNA. Only the agents active in the engine's mode take
                //    part; a due trigger opens an epoch. The allocation is
                //    published for the scheduler, which lends it at its next
                //    frame start: this thread never lends.
                let agents: Vec<Arc<std::sync::Mutex<dyn Agent>>> = {
                    let registry = registry.lock().unwrap_or_else(|e| e.into_inner());
                    registry.iter_for_mode(&ctx_copy.mode).cloned().collect()
                };
                epochs.observe(&agents);

                // A replay publishes the recorded allocations, one a tick, in
                // order, until the trace is exhausted.
                let replayed = replay.read().ok().and_then(|r| {
                    r.as_ref()
                        .and_then(|(t, c)| t.epochs.get(*c).map(|e| e.allocation.clone()))
                });
                if let Some(allocation) = replayed {
                    arbitrator.reissue(&allocation);
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
                    epochs.set_in_force(allocation.epoch);
                    // Once the trace runs out, a live epoch opens for the
                    // engine as it is now, whatever the trace lent last.
                    if exhausted {
                        epochs.raise(Trigger::Refresh);
                    }
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
                    // Per-agent measured costs anchor the agents' quotes in
                    // reality: each model read at its own agent's workload,
                    // else its latest observation.
                    let measured_costs: HashMap<AgentId, f64> = cost_models
                        .iter()
                        .filter_map(|(id, model)| {
                            workloads
                                .get(id)
                                .and_then(|n| model.predict_ms(*n))
                                .or_else(|| model.latest_ms())
                                .map(|ms| (*id, ms))
                        })
                        .collect();
                    arbitrator.set_wave_plan(&latest_wave_plan);
                    let allocation = arbitrator.allocate(
                        &ctx_copy,
                        &report,
                        trigger,
                        &agents,
                        &measured_costs,
                        &tick_hints,
                    );
                    epochs.opened(&agents);
                    epochs.set_in_force(allocation.epoch);
                    last_issued_multiplier = Some(multiplier);
                    if decision_recording.read().map(|b| *b).unwrap_or(false) {
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
                            });
                        }
                    }
                    if let Some(channel) = connected(&budget_channel) {
                        channel.publish(allocation);
                    }
                }

                // 4. Sleep until next tick
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

/// `map`'s entries in the agents' order, so a journal reads the same from one
/// run to the next.
fn by_agent<T: Clone>(map: &HashMap<AgentId, T>) -> Vec<(AgentId, T)> {
    let mut entries: Vec<(AgentId, T)> =
        map.iter().map(|(id, value)| (*id, value.clone())).collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}
