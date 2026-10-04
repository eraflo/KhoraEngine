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

use super::forecast::forecast_total_ms;
use super::{DccService, COST_MODEL_CAPACITY, FRAME_TIME_MIN_SAMPLES, PID_RENEGOTIATE_DELTA};
use crate::cost_model::CostModel;
use crate::dcc_context::safety_ceiling;
use crate::metrics::MetricStore;
use crate::pid::PidController;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::ResourceBudget;
use khora_core::agent::Agent;
use khora_core::telemetry::{MetricId, TelemetryEvent};
use khora_data::ecs::layout::LayoutAdvisor;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use khora_core::agent::gorna::{AgentHints, AgentId, TickDecisions};
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
        let budget_channel = self.budget_channel.clone();
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

        let handle = thread::spawn(move || {
            let mut store = MetricStore::new();
            let heuristic_engine = HeuristicEngine;
            let mut arbitrator = GornaArbitrator::new(agent_lock_timeout);
            let mut initial_negotiation_done = false;
            // Per-agent empirical cost models (`c·f(n)`), fed from `AgentCost`
            // samples and used to forecast budget breaches before they happen.
            let mut cost_models: HashMap<AgentId, CostModel> = HashMap::new();
            let mut last_workload_n: f64 = 0.0;
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
                            ctx.hardware.available_vram = report.gpu_timings.as_ref().map(|_| 0);

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
                        TelemetryEvent::AgentCost { id, n, time_ms } => {
                            // Feed the per-agent empirical cost model and surface
                            // the latest time as a glass-box metric.
                            last_workload_n = n;
                            cost_models
                                .entry(id)
                                .or_insert_with(|| CostModel::new(COST_MODEL_CAPACITY))
                                .record(n, time_ms);
                            store.push(
                                khora_core::telemetry::MetricId::new(
                                    "agent",
                                    format!("{id:?}_time_ms"),
                                ),
                                time_ms as f32,
                            );
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
                    forecast_total_ms(&cost_models, last_workload_n, &latest_wave_plan)
                {
                    if predicted_ms > report.suggested_latency_ms as f64 {
                        let ratio = (report.suggested_latency_ms as f64 / predicted_ms)
                            .clamp(0.5, 1.0) as f32;
                        report.alerts.push(format!(
                            "Cost-model forecast {:.1}ms > budget {:.1}ms at n={:.0} — tightening to {:.1}ms",
                            predicted_ms,
                            report.suggested_latency_ms,
                            last_workload_n,
                            report.suggested_latency_ms * ratio
                        ));
                        report.suggested_latency_ms *= ratio;
                        report.needs_negotiation = true;
                    }
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
                        report.needs_negotiation = true;
                        report.alerts.push(format!(
                            "PID: budget multiplier {last:.2} → {multiplier:.2} since last issuance — re-arbitrating",
                        ));
                    }
                }

                for alert in &report.alerts {
                    log::info!("DCC Analysis: {}", alert);
                }

                // 2c. Replay: while a trace is loaded, drive arbitration every
                //     tick from the recorded decisions (in order) until it is
                //     exhausted, then fall back to live negotiation.
                let replay_tick: Option<TickDecisions> = replay
                    .read()
                    .ok()
                    .and_then(|r| r.as_ref().and_then(|(t, c)| t.ticks.get(*c).cloned()));
                let replaying = replay_tick.is_some();

                // 3. GORNA Negotiation
                if report.needs_negotiation || !initial_negotiation_done || replaying {
                    let registry_lock = registry.lock().unwrap_or_else(|e| e.into_inner());
                    if !registry_lock.is_empty() {
                        let agents: Vec<_> = registry_lock.iter().cloned().collect();
                        drop(registry_lock);

                        let mut agents_slice: Vec<Arc<std::sync::Mutex<dyn Agent>>> = agents;
                        // Sync developer-control modes into the arbitrator before
                        // it issues budgets (host may have changed them).
                        if let Ok(modes) = adaptation_modes.read() {
                            for (id, mode) in modes.iter() {
                                arbitrator.set_adaptation_mode(*id, *mode);
                            }
                        }
                        // Snapshot the developer hints for this tick (Cap /
                        // Prioritize biases). Empty map = no hints = default
                        // behaviour, so this is bit-identical when unused.
                        let tick_hints: HashMap<AgentId, AgentHints> =
                            hints.read().map(|h| h.clone()).unwrap_or_default();
                        // Per-agent measured costs anchor the agents' self-quoted
                        // estimates in reality during fitting: prefer the model's
                        // forecast at the current workload, fall back to the
                        // latest raw observation when no fit is available yet.
                        let measured_costs: HashMap<AgentId, f64> = cost_models
                            .iter()
                            .filter_map(|(id, m)| {
                                m.predict_ms(last_workload_n)
                                    .or_else(|| m.latest_ms())
                                    .map(|ms| (*id, ms))
                            })
                            .collect();

                        // Give the arbitrator the current wave grouping so its
                        // budget fit costs concurrent waves by their critical
                        // path (empty plan = serial = sum-of-costs).
                        arbitrator.set_wave_plan(&latest_wave_plan);
                        let issued = arbitrator.arbitrate(
                            &ctx_copy,
                            &report,
                            &mut agents_slice,
                            &measured_costs,
                            replay_tick.as_ref(),
                            &tick_hints,
                        );
                        initial_negotiation_done = true;
                        last_issued_multiplier = Some(multiplier);

                        // Advance the replay cursor, or record this tick's decisions.
                        if replaying {
                            if let Ok(mut r) = replay.write() {
                                if let Some((_, cursor)) = r.as_mut() {
                                    *cursor += 1;
                                }
                            }
                        } else if decision_recording.read().map(|b| *b).unwrap_or(false) {
                            if let Ok(mut trace) = recorded_trace.write() {
                                trace.ticks.push(issued);
                            }
                        }

                        // Send budgets through the budget channel to the Scheduler.
                        if let Some(ref budget_channel) = budget_channel {
                            for agent_arc in &agents_slice {
                                if let Ok(agent) = agent_arc.lock() {
                                    // The agent's current budget was set by apply_budget() during arbitration.
                                    // We need to reconstruct the budget from the agent's status.
                                    let status = agent.report_status();
                                    let budget = ResourceBudget {
                                        strategy_id: status.current_strategy,
                                        time_limit: Duration::from_secs_f32(
                                            report.suggested_latency_ms / 1000.0,
                                        ),
                                        memory_limit: None,
                                        extra_params: std::collections::HashMap::new(),
                                    };
                                    budget_channel.send(agent.id(), budget);
                                }
                            }
                        }
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
