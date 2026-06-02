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

//! Central service for the Dynamic Context Core.

use crate::budget_channel::BudgetChannel;
use crate::context::{safety_ceiling, Context};
use crate::cost_model::CostModel;
use crate::metrics::MetricStore;
use crate::EngineMode;
use crossbeam_channel::{Receiver, Sender};
use khora_core::agent::Agent;
use khora_core::control::gorna::ResourceBudget;
use khora_core::control::pid::{PidConfig, PidController};
use khora_core::telemetry::{MetricId, TelemetryEvent};
use khora_data::ecs::layout::{LayoutAdvisor, LayoutRecommendation};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use crate::registry::AgentRegistry;
use khora_core::control::gorna::{AdaptationMode, AgentId, DecisionTrace, TickDecisions};
use std::collections::HashMap;
use std::sync::Mutex;

/// Number of recent `(n, time)` samples each per-agent cost model retains.
const COST_MODEL_CAPACITY: usize = 64;

/// Minimum frame-time samples before the PID acts on the measurement. Below this
/// the controller holds its current output rather than reacting to thin data.
const FRAME_TIME_MIN_SAMPLES: usize = 10;

/// Sums each agent's empirically-forecast cost (`c·f(n)`) at workload `n`.
///
/// Returns `None` until at least one agent has enough distinct-`n` samples to
/// fit a model — before that the DCC has nothing to anticipate with.
fn forecast_total_ms(models: &HashMap<AgentId, CostModel>, n: f64) -> Option<f64> {
    let mut total = 0.0;
    let mut any = false;
    for m in models.values() {
        if let Some(p) = m.predict_ms(n) {
            total += p.max(0.0);
            any = true;
        }
    }
    any.then_some(total)
}

/// Configuration for the DCC Service.
#[derive(Debug, Clone)]
pub struct DccConfig {
    /// Frequency of the analysis loop in Hz.
    pub tick_rate: u32,
    /// Maximum number of telemetry events to buffer.
    /// If the buffer is full, new events are dropped.
    pub telemetry_buffer_size: usize,
    /// Timeout for acquiring locks on agents during negotiation.
    /// If an agent lock cannot be acquired within this time, the agent is skipped.
    pub agent_lock_timeout_ms: u64,
    /// Optional system-RAM budget in bytes. When set, the DCC derives
    /// [`Context::memory_pressure`] from the tracking allocator's live usage and
    /// degrades frame budgets as the ceiling approaches. `None` disables the
    /// signal — chiefly useful on memory-constrained targets.
    pub memory_budget_bytes: Option<u64>,
    /// Tuning for the frame-time PID that drives [`Context::global_budget_multiplier`].
    /// The defaults are conservative gains for the ~20 Hz cold path; expose this
    /// to retune per target without touching the loop.
    pub frame_pid: PidConfig,
}

impl Default for DccConfig {
    fn default() -> Self {
        Self {
            tick_rate: 20,
            telemetry_buffer_size: 1000,
            agent_lock_timeout_ms: 100,
            memory_budget_bytes: None,
            frame_pid: PidConfig::default(),
        }
    }
}

/// The Dynamic Context Core service.
///
/// Manages the cold-path analysis loop, GORNA arbitration, and agent coordination.
pub struct DccService {
    config: DccConfig,
    context: Arc<std::sync::RwLock<Context>>,
    registry: Arc<std::sync::Mutex<AgentRegistry>>,
    budget_channel: Option<BudgetChannel>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    event_tx: Sender<TelemetryEvent>,
    /// Per-agent developer-control modes, shared with the cold-path arbitrator.
    /// Written from any thread (host/editor), read each tick by the DCC loop.
    adaptation_modes: Arc<std::sync::RwLock<HashMap<AgentId, AdaptationMode>>>,
    /// Latest read-only layout recommendations per component, derived by the
    /// DCC from access telemetry via the Data-layer advisor. Glass-box only:
    /// the DCC *advises*, it never repacks — Data owns its layout (CLAD).
    layout_recommendations: Arc<std::sync::RwLock<HashMap<String, LayoutRecommendation>>>,
    /// Whether the DCC is recording GORNA decisions for deterministic replay.
    decision_recording: Arc<std::sync::RwLock<bool>>,
    /// The accumulating recorded decision trace (one entry per arbitration tick).
    recorded_trace: Arc<std::sync::RwLock<DecisionTrace>>,
    /// When `Some((trace, cursor))`, arbitration replays the trace tick by tick
    /// instead of negotiating live.
    replay: Arc<std::sync::RwLock<Option<(DecisionTrace, usize)>>>,
}

impl DccService {
    /// Creates a new DCC service.
    pub fn new(config: DccConfig) -> (Self, Receiver<TelemetryEvent>) {
        let (tx, rx) = crossbeam_channel::bounded(config.telemetry_buffer_size);
        let service = Self {
            config,
            context: Arc::new(std::sync::RwLock::new(Context::default())),
            registry: Arc::new(std::sync::Mutex::new(AgentRegistry::new())),
            budget_channel: None,
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            event_tx: tx,
            adaptation_modes: Arc::new(std::sync::RwLock::new(HashMap::new())),
            layout_recommendations: Arc::new(std::sync::RwLock::new(HashMap::new())),
            decision_recording: Arc::new(std::sync::RwLock::new(false)),
            recorded_trace: Arc::new(std::sync::RwLock::new(DecisionTrace::default())),
            replay: Arc::new(std::sync::RwLock::new(None)),
        };
        (service, rx)
    }

    /// Sets the [`AdaptationMode`] for an agent — the developer-control surface
    /// over the adaptive core. Thread-safe; takes effect on the next arbitration
    /// tick. `Manual(strategy)` pins an agent, `Stable` blocks opportunistic
    /// upgrades, `Bounded` clamps the range, `Learning` (default) negotiates freely.
    pub fn set_adaptation_mode(&self, agent_id: AgentId, mode: AdaptationMode) {
        if let Ok(mut modes) = self.adaptation_modes.write() {
            modes.insert(agent_id, mode);
        }
    }

    /// Returns the [`AdaptationMode`] configured for an agent (default `Learning`).
    pub fn adaptation_mode(&self, agent_id: AgentId) -> AdaptationMode {
        self.adaptation_modes
            .read()
            .ok()
            .and_then(|m| m.get(&agent_id).copied())
            .unwrap_or_default()
    }

    /// Read-only snapshot of the per-component layout recommendations the DCC's
    /// advisor has derived from access telemetry — for the glass-box surface
    /// (e.g. the editor's Control-Plane panel). Advisory only: the DCC observes
    /// the Data layer and recommends, it never repacks.
    pub fn layout_recommendations(&self) -> HashMap<String, LayoutRecommendation> {
        self.layout_recommendations
            .read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Starts recording GORNA's per-tick decisions into a fresh trace. The
    /// recording can be replayed later for bit-for-bit reproduction (QA,
    /// network lockstep, bug repro). Thread-safe; takes effect next tick.
    pub fn start_decision_recording(&self) {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = true;
        }
        if let Ok(mut trace) = self.recorded_trace.write() {
            *trace = DecisionTrace::default();
        }
    }

    /// Stops recording and returns the captured [`DecisionTrace`].
    pub fn stop_decision_recording(&self) -> DecisionTrace {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = false;
        }
        self.recorded_decisions()
    }

    /// A snapshot of the decisions recorded so far (glass-box; safe any time).
    pub fn recorded_decisions(&self) -> DecisionTrace {
        self.recorded_trace
            .read()
            .map(|t| t.clone())
            .unwrap_or_default()
    }

    /// Begins replaying `trace`: each subsequent arbitration tick issues the
    /// recorded strategies in order (bypassing live fit + `AdaptationMode`)
    /// until the trace is exhausted, then normal arbitration resumes.
    pub fn replay_decisions(&self, trace: DecisionTrace) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = Some((trace, 0));
        }
    }

    /// Stops any in-progress replay, returning to live arbitration.
    pub fn stop_replay(&self) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = None;
        }
    }

    /// Whether a replay is currently in progress (trace not yet exhausted).
    pub fn is_replaying(&self) -> bool {
        self.replay
            .read()
            .ok()
            .and_then(|r| r.as_ref().map(|(t, c)| *c < t.ticks.len()))
            .unwrap_or(false)
    }

    /// Connects the DCC to the Scheduler's budget channel.
    /// After GORNA arbitration, budgets are sent through this channel.
    pub fn connect_budget_channel(&mut self, channel: BudgetChannel) {
        self.budget_channel = Some(channel);
    }

    /// Registers an agent with a priority value.
    ///
    /// Higher priority values mean the agent is updated first in each frame.
    /// The agent is active in all engine modes.
    pub fn register_agent(&self, agent: Arc<std::sync::Mutex<dyn Agent>>, priority: f32) {
        let mut registry = self.registry.lock().unwrap();
        registry.register(agent, priority);
    }

    /// Registers an agent with a priority value, active only in the specified modes.
    ///
    /// Higher priority values mean the agent is updated first in each frame.
    /// If `modes` is empty, the agent is active in all modes.
    pub fn register_agent_for_mode(
        &self,
        agent: Arc<std::sync::Mutex<dyn Agent>>,
        priority: f32,
        modes: Vec<EngineMode>,
    ) {
        let mut registry = self.registry.lock().unwrap();
        registry.register_for_mode(agent, priority, modes);
    }

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
            // Read-only layout advisor: turns per-component access telemetry into
            // a recommendation for the glass-box. Stateless (a tuned heuristic).
            let layout_advisor = LayoutAdvisor::default();
            // Frame-time PID: regulates the global budget multiplier so measured
            // frame time tracks the heuristic-suggested latency. Persists across
            // ticks; `last_tick` gives the real `dt` between updates.
            let mut frame_pid = PidController::new(frame_pid_cfg);
            let mut last_tick: Option<Instant> = None;

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
                            let mut ctx = context.write().unwrap();
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
                        TelemetryEvent::PhaseChange(phase_name) => {
                            let mut ctx = context.write().unwrap();
                            if let Some(new_mode) = EngineMode::from_name(&phase_name) {
                                log::debug!("DCC Mode: {:?} → {:?}", ctx.mode, new_mode);
                                ctx.mode = new_mode;
                            } else {
                                log::warn!("DCC: Unknown mode '{}'", phase_name);
                            }
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
                    let mut ctx = context.write().unwrap();
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
                if let Some(predicted_ms) = forecast_total_ms(&cost_models, last_workload_n) {
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
                    let registry_lock = registry.lock().unwrap();
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
                        let issued = arbitrator.arbitrate(
                            &ctx_copy,
                            &report,
                            &mut agents_slice,
                            replay_tick.as_ref(),
                        );
                        initial_negotiation_done = true;

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

    /// Returns the agent registry for use by the Scheduler.
    pub fn agent_registry(&self) -> &Arc<Mutex<AgentRegistry>> {
        &self.registry
    }

    /// Returns a sender handle to submit events to the DCC.
    pub fn event_sender(&self) -> Sender<TelemetryEvent> {
        self.event_tx.clone()
    }

    /// Returns the current context.
    pub fn get_context(&self) -> Context {
        self.context.read().unwrap().clone()
    }

    /// Returns a shared handle to the live context.
    ///
    /// Used by observers (e.g. the editor's Control Plane workspace) that
    /// want to read up-to-date hardware state, mode, and budget multiplier
    /// each frame without going through `get_context()`'s clone.
    pub fn context_handle(&self) -> Arc<std::sync::RwLock<Context>> {
        Arc::clone(&self.context)
    }

    /// Initializes all registered agents once after registration.
    ///
    /// Should be called once after all agents are registered, giving them
    /// access to engine services for caching and lane setup.
    pub fn initialize_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(registry) = self.registry.lock() {
            registry.initialize_all(context);
        }
    }

    /// Executes all registered agents in priority order.
    ///
    /// Called each frame. Each agent selects the appropriate lanes and
    /// dispatches their execution.
    pub fn execute_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(registry) = self.registry.lock() {
            registry.execute_all(context);
        }
    }

    /// Returns the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.registry.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Returns a reference to the agent with the given ID, if registered.
    pub fn get_agent(&self, id: AgentId) -> Option<Arc<Mutex<dyn Agent>>> {
        self.registry.lock().ok()?.get_by_id(id)
    }
}

impl Drop for DccService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineMode;
    use khora_core::control::gorna::{
        AdaptationMode, AgentId, AgentStatus, NegotiationRequest, NegotiationResponse,
        ResourceBudget, StrategyId, StrategyOption,
    };
    use khora_core::telemetry::{MetricId, MetricValue};

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
        assert!(forecast_total_ms(&models, 100.0).is_none());

        // Renderer: linear 3·n; Physics: linear 2·n.
        let mut renderer = CostModel::new(16);
        let mut physics = CostModel::new(16);
        for n in [10.0, 20.0, 40.0] {
            renderer.record(n, 3.0 * n);
            physics.record(n, 2.0 * n);
        }
        models.insert(AgentId::Renderer, renderer);
        models.insert(AgentId::Physics, physics);

        // Forecast at n=100 → 300 + 200 = 500ms (within fit tolerance).
        let total = forecast_total_ms(&models, 100.0).expect("a fit is available");
        assert!((total - 500.0).abs() < 1.0, "forecast = {total}");
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
}
