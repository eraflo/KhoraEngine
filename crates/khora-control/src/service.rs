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
use khora_core::telemetry::TelemetryEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::analysis::HeuristicEngine;
use crate::gorna::GornaArbitrator;
use khora_core::agent::Agent;

/// Configuration for the DCC Service.
#[derive(Debug, Clone)]
pub struct DccConfig {
    /// frequency of the analysis loop in Hz.
    pub tick_rate: u32,
}

impl Default for DccConfig {
    fn default() -> Self {
        Self { tick_rate: 20 } // 20Hz is a good balance for cold-path analysis
    }
}

/// The Dynamic Context Core service.
pub struct DccService {
    config: DccConfig,
    context: Arc<std::sync::RwLock<Context>>,
    agents: Arc<std::sync::Mutex<Vec<Arc<std::sync::Mutex<dyn Agent>>>>>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    event_tx: Sender<TelemetryEvent>,
}

impl DccService {
    /// Creates a new DCC service.
    pub fn new(config: DccConfig) -> (Self, Receiver<TelemetryEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let service = Self {
            config,
            context: Arc::new(std::sync::RwLock::new(Context::default())),
            agents: Arc::new(std::sync::Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            event_tx: tx,
        };
        (service, rx)
    }

    /// Registers an Intelligent Subsystem Agent with the DCC.
    pub fn register_agent(&self, agent: Arc<std::sync::Mutex<dyn Agent>>) {
        let mut agents = self.agents.lock().unwrap();
        {
            let a = agent.lock().unwrap();
            log::info!("DCC: Registered agent {:?}", a.id());
        }
        agents.push(agent);
    }

    /// Starts the DCC background thread.
    pub fn start(&mut self, event_rx: Receiver<TelemetryEvent>) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let context = Arc::clone(&self.context);
        let agents = Arc::clone(&self.agents);
        let tick_duration = Duration::from_secs_f32(1.0 / self.config.tick_rate as f32);

        let handle = thread::spawn(move || {
            let mut store = MetricStore::new();
            let heuristic_engine = HeuristicEngine;
            let arbitrator = GornaArbitrator;

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
                        TelemetryEvent::ResourceReport(_) => {
                            // Memory/VRAM reports are handled via individual metrics for now
                        }
                        TelemetryEvent::HardwareReport(report) => {
                            let mut ctx = context.write().unwrap();
                            ctx.hardware.thermal = report.thermal;
                            ctx.hardware.battery = report.battery;
                            ctx.hardware.cpu_load = report.cpu_load;
                            ctx.hardware.gpu_load = report.gpu_load.unwrap_or(0.0);
                            // Eagerly update the budget multiplier on hardware changes.
                            ctx.refresh_budget_multiplier();

                            // Extract frame time metrics if available
                            if let Some(gpu_timings) = report.gpu_timings {
                                if let Some(frame_time_us) = gpu_timings.frame_total_duration_us() {
                                    store.push(
                                        khora_core::telemetry::MetricId::new(
                                            "renderer",
                                            "frame_time",
                                        ),
                                        frame_time_us as f32 / 1000.0, // Convert to ms
                                    );
                                }
                            }

                            log::debug!(
                                "DCC Hardware updated: Thermal={:?}, CPU={:.2}, GPU={:?}",
                                ctx.hardware.thermal,
                                ctx.hardware.cpu_load,
                                ctx.hardware.gpu_load
                            );
                        }
                        TelemetryEvent::PhaseChange(phase_name) => {
                            let mut ctx = context.write().unwrap();
                            ctx.phase = match phase_name.to_lowercase().as_str() {
                                "boot" => ExecutionPhase::Boot,
                                "menu" => ExecutionPhase::Menu,
                                "simulation" => ExecutionPhase::Simulation,
                                "background" => ExecutionPhase::Background,
                                _ => ctx.phase,
                            };
                            log::debug!("DCC Phase changed to: {:?}", ctx.phase);
                        }
                        TelemetryEvent::GpuReport(report) => {
                            // Ingest GPU timing data into the metric store.
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
                                khora_core::telemetry::MetricId::new(
                                    "renderer",
                                    "draw_calls",
                                ),
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
                    // Recompute global budget multiplier from latest hardware state.
                    ctx.refresh_budget_multiplier();
                    let report = heuristic_engine.analyze(&ctx, &store);
                    (report, ctx.clone())
                };

                // Log analysis alerts for observability.
                for alert in &report.alerts {
                    log::info!("DCC Analysis: {}", alert);
                }

                if report.needs_negotiation {
                    let mut agents_lock = agents.lock().unwrap();
                    arbitrator.arbitrate(&ctx_copy, &report, &mut agents_lock);
                }

                // 3. Sleep until next tick
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

    /// Periodically updates the engine's situational model.
    pub fn get_context(&self) -> Context {
        self.context.read().unwrap().clone()
    }

    /// Triggers the tactical update phase for all registered agents.
    /// This should be called by the engine loop (e.g. in the redraw request).
    pub fn update_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(mut agents) = self.agents.lock() {
            for agent_mutex in agents.iter_mut() {
                if let Ok(mut agent) = agent_mutex.lock() {
                    agent.update(context);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::telemetry::MetricId;
    use khora_core::telemetry::MetricValue;

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

        // Give some time for the thread to process
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

        // Smoke test: should not panic/hang
        thread::sleep(Duration::from_millis(50));

        dcc.stop();
    }
}

impl Drop for DccService {
    fn drop(&mut self) {
        self.stop();
    }
}
