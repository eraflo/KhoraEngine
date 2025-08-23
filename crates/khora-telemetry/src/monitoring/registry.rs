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

//! Registry for managing resource monitors.

use khora_core::telemetry::ResourceMonitor;
use std::sync::{Arc, Mutex};

/// A thread-safe registry for resource monitors.
#[derive(Debug, Clone)]
pub struct MonitorRegistry {
    monitors: Arc<Mutex<Vec<Arc<dyn ResourceMonitor>>>>,
}

impl MonitorRegistry {
    /// Creates a new, empty monitor registry.
    pub fn new() -> Self {
        Self {
            monitors: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Registers a new resource monitor.
    pub fn register(&self, monitor: Arc<dyn ResourceMonitor>) {
        let mut monitors_guard = self.monitors.lock().unwrap();
        let monitor_id = monitor.monitor_id().to_string();
        monitors_guard.push(monitor);
        log::info!("Registered resource monitor: {}", monitor_id);
    }

    /// Calls the `update` method on all registered monitors.
    pub fn update_all(&self) {
        let monitors_guard = self.monitors.lock().unwrap();
        for monitor in monitors_guard.iter() {
            monitor.update();
        }
    }

    /// Returns a clone of all registered monitors.
    pub fn get_all_monitors(&self) -> Vec<Arc<dyn ResourceMonitor>> {
        self.monitors.lock().unwrap().clone()
    }
}

impl Default for MonitorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
