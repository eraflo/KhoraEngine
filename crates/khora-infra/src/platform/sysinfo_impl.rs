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

//! sysinfo-based implementation of the HardwareMonitor trait.

use khora_core::platform::{BatteryLevel, HardwareMonitor, ThermalStatus};
use std::sync::{Arc, Mutex};
use sysinfo::{Components, System};

/// A hardware monitor that uses the `sysinfo` crate.
pub struct SysinfoMonitor {
    system: Arc<Mutex<System>>,
}

impl SysinfoMonitor {
    /// Creates a new SysinfoMonitor.
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        Self {
            system: Arc::new(Mutex::new(system)),
        }
    }

    /// Refreshes the underlying system data.
    pub fn refresh(&self) {
        if let Ok(mut system) = self.system.lock() {
            system.refresh_cpu_all();
            // refresh_components is now handled by new_with_refreshed_list in thermal_status or similar
        }
    }
}

impl HardwareMonitor for SysinfoMonitor {
    fn thermal_status(&self) -> ThermalStatus {
        let components = Components::new_with_refreshed_list();
        let mut max_temp = 0.0;

        for component in &components {
            let label = component.label().to_lowercase();
            if label.contains("cpu") || label.contains("core") {
                if let Some(temp) = component.temperature() {
                    max_temp = f32::max(max_temp, temp);
                }
            }
        }

        if max_temp == 0.0 {
            return ThermalStatus::Cool; // Unknown or unavailable
        }

        if max_temp > 90.0 {
            ThermalStatus::Critical
        } else if max_temp > 80.0 {
            ThermalStatus::Throttling
        } else if max_temp > 60.0 {
            ThermalStatus::Warm
        } else {
            ThermalStatus::Cool
        }
    }

    fn battery_level(&self) -> BatteryLevel {
        // sysinfo doesn't easily expose battery on all platforms via System.
        // For v0.1 we return Mains.
        BatteryLevel::Mains
    }

    fn cpu_load(&self) -> f32 {
        if let Ok(system) = self.system.lock() {
            system.global_cpu_usage() / 100.0
        } else {
            0.0
        }
    }
}

impl Default for SysinfoMonitor {
    fn default() -> Self {
        Self::new()
    }
}
