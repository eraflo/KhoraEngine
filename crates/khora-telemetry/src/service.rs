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
use std::time::{Duration, Instant};

/// Service for managing telemetry data and resource monitoring.
#[derive(Debug)]
pub struct TelemetryService {
    metrics: MetricsRegistry,
    monitors: MonitorRegistry,
    last_update: Instant,
    update_interval: Duration,
}

impl TelemetryService {
    /// Creates a new telemetry service with the given update interval.
    pub fn new(update_interval: Duration) -> Self {
        Self {
            metrics: MetricsRegistry::new(),
            monitors: MonitorRegistry::new(),
            last_update: Instant::now(),
            update_interval,
        }
    }

    /// Should be called periodically (e.g., once per frame).
    /// Updates all registered resource monitors if the interval has passed.
    pub fn tick(&mut self) -> bool {
        if self.last_update.elapsed() >= self.update_interval {
            log::trace!("Updating all resource monitors...");
            self.monitors.update_all();
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
