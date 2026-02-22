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

use crate::context::{Context, ExecutionPhase};
use crate::metrics::MetricStore;
use crossbeam_channel::{Receiver, Sender};
use khora_core::agent::Agent;
use khora_core::telemetry::TelemetryEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use crate::registry::AgentRegistry;
use khora_core::control::gorna::AgentId;
use std::sync::Mutex;

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
}

impl Default for DccConfig {
    fn default() -> Self {
        Self {
            tick_rate: 20,
            telemetry_buffer_size: 1000,
            agent_lock_timeout_ms: 100,
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
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    event_tx: Sender<TelemetryEvent>,
}

impl DccService {
    /// Creates a new DCC service.
    pub fn new(config: DccConfig) -> (Self, Receiver<TelemetryEvent>) {
        let (tx, rx) = crossbeam_channel::bounded(config.telemetry_buffer_size);
        let service = Self {
            config,
            context: Arc::new(std::sync::RwLock::new(Context::default())),
            registry: Arc::new(std::sync::Mutex::new(AgentRegistry::new())),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            event_tx: tx,
        };
        (service, rx)
    }

    /// Registers an agent with a priority value.
    ///
    /// Higher priority values mean the agent is updated first in each frame.
    pub fn register_agent(&self, agent: Arc<std::sync::Mutex<dyn Agent>>, priority: f32) {
        let mut registry = self.registry.lock().unwrap();
        registry.register(agent, priority);
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
        let tick_duration = Duration::from_secs_f32(1.0 / self.config.tick_rate as f32);
        let agent_lock_timeout = Duration::from_millis(self.config.agent_lock_timeout_ms);

        let handle = thread::spawn(move || {
            let mut store = MetricStore::new();
            let heuristic_engine = HeuristicEngine;
            let arbitrator = GornaArbitrator::new(agent_lock_timeout);
            let mut initial_negotiation_done = false;

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
                            ctx.refresh_budget_multiplier();

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
                            if let Some(new_phase) = ExecutionPhase::from_name(&phase_name) {
                                if ctx.phase.can_transition_to(new_phase) {
                                    log::debug!("DCC Phase: {:?} → {:?}", ctx.phase, new_phase);
                                    ctx.phase = new_phase;
                                } else {
                                    log::warn!(
                                        "DCC: Invalid transition {:?} → {:?}",
                                        ctx.phase,
                                        new_phase
                                    );
                                }
                            } else {
                                log::warn!("DCC: Unknown phase '{}'", phase_name);
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
                    }
                }

                // 2. Perform Analysis & Arbitration
                let (report, ctx_copy) = {
                    let mut ctx = context.write().unwrap();
                    ctx.refresh_budget_multiplier();
                    let report = heuristic_engine.analyze(&ctx, &store);
                    (report, ctx.clone())
                };

                for alert in &report.alerts {
                    log::info!("DCC Analysis: {}", alert);
                }

                // 3. GORNA Negotiation
                if report.needs_negotiation || !initial_negotiation_done {
                    let registry_lock = registry.lock().unwrap();
                    if !registry_lock.is_empty() {
                        let agents: Vec<_> = registry_lock.iter().cloned().collect();
                        drop(registry_lock);

                        let mut agents_slice: Vec<Arc<std::sync::Mutex<dyn Agent>>> = agents;
                        arbitrator.arbitrate(&ctx_copy, &report, &mut agents_slice);
                        initial_negotiation_done = true;
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

    /// Returns a sender handle to submit events to the DCC.
    pub fn event_sender(&self) -> Sender<TelemetryEvent> {
        self.event_tx.clone()
    }

    /// Returns the current context.
    pub fn get_context(&self) -> Context {
        self.context.read().unwrap().clone()
    }

    /// Updates all registered agents in priority order.
    ///
    /// This is called each frame by the engine loop.
    pub fn update_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(registry) = self.registry.lock() {
            registry.update_all(context);
        }
    }

    /// Executes all registered agents in priority order.
    ///
    /// Called each frame after [`update_agents`](Self::update_agents).
    /// Each agent performs its primary work (e.g., the `RenderAgent` renders).
    pub fn execute_agents(&self) {
        if let Ok(registry) = self.registry.lock() {
            registry.execute_all();
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
    use khora_core::control::gorna::{
        AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
        StrategyOption,
    };
    use khora_core::telemetry::{MetricId, MetricValue};

    struct StubAgent {
        budget_applied: bool,
    }

    impl Agent for StubAgent {
        fn id(&self) -> AgentId {
            AgentId::Renderer
        }
        fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
            NegotiationResponse {
                strategies: vec![StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_millis(8),
                    estimated_vram: 1024,
                }],
            }
        }
        fn apply_budget(&mut self, _: ResourceBudget) {
            self.budget_applied = true;
        }
        fn update(&mut self, _: &mut khora_core::EngineContext<'_>) {}
        fn report_status(&self) -> AgentStatus {
            AgentStatus {
                agent_id: AgentId::Renderer,
                current_strategy: StrategyId::Balanced,
                health_score: 1.0,
                is_stalled: false,
                message: String::new(),
            }
        }
        fn execute(&mut self) {}
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
        assert_eq!(ctx.phase, ExecutionPhase::Simulation);

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
    fn test_dcc_initial_negotiation_fires_with_agent() {
        let (mut dcc, rx) = DccService::new(DccConfig {
            tick_rate: 100,
            ..Default::default()
        });
        let agent = Arc::new(std::sync::Mutex::new(StubAgent {
            budget_applied: false,
        }));
        dcc.register_agent(agent.clone(), 1.0);
        dcc.start(rx);

        thread::sleep(Duration::from_millis(200));

        let applied = agent.lock().unwrap().budget_applied;
        dcc.stop();

        assert!(
            applied,
            "Initial GORNA negotiation should have called apply_budget"
        );
    }
}
