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

//! Service for managing telemetry data and resource monitoring.

use crate::metrics::registry::MetricsRegistry;
use crate::monitoring::registry::MonitorRegistry;
use crossbeam_channel::Sender;
use khora_core::telemetry::event::TelemetryEvent;
use std::time::{Duration, Instant};

/// Central service for collecting and managing engine-wide telemetry.
///
/// The `TelemetryService` acts as a central registry for all metrics and
/// resource monitors. It periodically triggers monitor updates and,
/// if configured, forwards the results to the DCC for higher-level analysis.
#[derive(Debug)]
pub struct TelemetryService {
    metrics: MetricsRegistry,
    monitors: MonitorRegistry,
    last_update: Instant,
    update_interval: Duration,
    /// Optional sender to forward events to the DCC.
    dcc_sender: Option<Sender<TelemetryEvent>>,
}

impl TelemetryService {
    /// Creates a new `TelemetryService` with the specified update interval.
    pub fn new(update_interval: Duration) -> Self {
        Self {
            metrics: MetricsRegistry::new(),
            monitors: MonitorRegistry::new(),
            last_update: Instant::now(),
            update_interval,
            dcc_sender: None,
        }
    }

    /// Sets the sender for forwarding events to the DCC.
    pub fn with_dcc_sender(mut self, sender: Sender<TelemetryEvent>) -> Self {
        self.dcc_sender = Some(sender);
        self
    }

    /// Updates all registered monitors if the update interval has passed.
    ///
    /// Returns `true` if monitors were updated, `false` otherwise.
    pub fn tick(&mut self) -> bool {
        if self.last_update.elapsed() >= self.update_interval {
            log::trace!("Updating all resource monitors...");
            self.monitors.update_all();

            // Forward monitor reports to DCC if sender is configured.
            if let Some(sender) = &self.dcc_sender {
                // 1. Forward monitor reports.
                for monitor in self.monitors.get_all_monitors() {
                    // Standard ResourceUsageReport (bytes)
                    let report = monitor.get_usage_report();
                    let _ = sender.send(TelemetryEvent::ResourceReport(report));

                    // GPU Performance Report (timings)
                    if let Some(gpu_report) = monitor.get_gpu_report() {
                        let _ = sender.send(TelemetryEvent::GpuReport(gpu_report));
                    }

                    // Hardware Health Report (thermal, load)
                    if let Some(hw_report) = monitor.get_hardware_report() {
                        let _ = sender.send(TelemetryEvent::HardwareReport(hw_report));
                    }

                    // Discrete Metrics
                    for (id, value) in monitor.get_metrics() {
                        let _ = sender.send(TelemetryEvent::MetricUpdate { id, value });
                    }
                }

                // 2. Forward metric updates.
                for metric in self.metrics.backend().list_all_metrics() {
                    let _ = sender.send(TelemetryEvent::MetricUpdate {
                        id: metric.metadata.id,
                        value: metric.value,
                    });
                }
            }

            self.last_update = Instant::now();
            true
        } else {
            false
        }
    }

    /// Returns a reference to the metrics registry.
    pub fn metrics_registry(&self) -> &MetricsRegistry {
        &self.metrics
    }

    /// Returns a reference to the monitor registry.
    pub fn monitor_registry(&self) -> &MonitorRegistry {
        &self.monitors
    }
}

impl Default for TelemetryService {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}
