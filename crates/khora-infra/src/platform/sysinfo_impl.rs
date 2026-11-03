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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::{Components, System};

/// The battery level one battery reports, from its `state` and its `charge`
/// (a fraction in `[0, 1]`): on mains (charging, full, paused at a charge
/// limit) it is [`BatteryLevel::Mains`]; discharging, `High` above 50 %,
/// `Low` above 15 %, `Critical` at or under 15 %.
pub fn battery_level_of(state: starship_battery::State, charge: f32) -> BatteryLevel {
    use starship_battery::State;
    match state {
        State::Charging | State::Full | State::Paused => BatteryLevel::Mains,
        State::Empty => BatteryLevel::Critical,
        // A state the platform could not name says nothing of the plug: the
        // charge decides, as for a discharging battery.
        State::Discharging | State::Unknown => {
            if charge > 0.5 {
                BatteryLevel::High
            } else if charge > 0.15 {
                BatteryLevel::Low
            } else {
                BatteryLevel::Critical
            }
        }
    }
}

/// The level of the machine's batteries: [`BatteryLevel::Mains`] without a
/// battery or when one is on mains, else the lowest of them; `Mains` too when
/// the platform cannot be asked, said once.
fn batteries_level(warned: &AtomicBool) -> BatteryLevel {
    let batteries = starship_battery::Manager::new().and_then(|manager| manager.batteries());
    let batteries = match batteries {
        Ok(batteries) => batteries,
        Err(error) => {
            if !warned.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "hardware monitor: the batteries cannot be read ({error}); taken as on mains"
                );
            }
            return BatteryLevel::Mains;
        }
    };
    let levels: Vec<BatteryLevel> = batteries
        .flatten()
        .map(|battery| {
            let charge = battery
                .state_of_charge()
                .get::<starship_battery::units::ratio::ratio>();
            battery_level_of(battery.state(), charge)
        })
        .collect();
    if levels.is_empty() || levels.contains(&BatteryLevel::Mains) {
        return BatteryLevel::Mains;
    }
    [
        BatteryLevel::Critical,
        BatteryLevel::Low,
        BatteryLevel::High,
    ]
    .into_iter()
    .find(|level| levels.contains(level))
    .unwrap_or(BatteryLevel::Mains)
}

/// A hardware monitor that uses the `sysinfo` crate.
///
/// The engine's hardware monitor, read by the DCC's thread: heat, battery,
/// the CPU's load — the situation the DCC's heuristics shape the frame
/// target from. Reading the sensors is slow (milliseconds, more the first
/// time), which is why it is never read on the frame's thread.
#[derive(Debug)]
pub struct SysinfoMonitor {
    system: Arc<Mutex<System>>,
    /// The batteries' level as the last [`refresh`](HardwareMonitor::refresh)
    /// read it.
    battery: Mutex<BatteryLevel>,
    /// Whether the batteries could not be read and it was said.
    battery_warned: AtomicBool,
}

impl SysinfoMonitor {
    /// A monitor that has read the CPU once. It reads no process and no
    /// disk: the hardware's health is all it is asked.
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_usage();
        Self {
            system: Arc::new(Mutex::new(system)),
            battery: Mutex::new(BatteryLevel::Mains),
            battery_warned: AtomicBool::new(false),
        }
    }
}

impl HardwareMonitor for SysinfoMonitor {
    /// Reads the CPU's load and the batteries again.
    fn refresh(&self) {
        if let Ok(mut system) = self.system.lock() {
            system.refresh_cpu_usage();
        }
        let level = batteries_level(&self.battery_warned);
        *self.battery.lock().unwrap_or_else(|e| e.into_inner()) = level;
    }

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
        *self.battery.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cpu_load(&self) -> f32 {
        if let Ok(system) = self.system.lock() {
            (system.global_cpu_usage() / 100.0).clamp(0.0, 1.0)
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

#[cfg(test)]
mod tests {
    use super::*;
    use starship_battery::State;

    /// **A battery on mains is mains, whatever its charge.** Charging, full,
    /// or paused at a charge limit, the machine draws from the wall: the
    /// level is `Mains`, even nearly empty.
    #[test]
    fn a_battery_on_mains_is_mains_whatever_its_charge() {
        for state in [State::Charging, State::Full, State::Paused] {
            for charge in [0.05, 0.15, 0.5, 1.0] {
                assert_eq!(
                    battery_level_of(state, charge),
                    BatteryLevel::Mains,
                    "{state:?} at {charge}"
                );
            }
        }
    }

    /// **A discharging battery is high above half.** Over 50 % it is `High`;
    /// exactly 50 % is no longer above half, so `Low`.
    #[test]
    fn a_discharging_battery_is_high_above_half() {
        assert_eq!(
            battery_level_of(State::Discharging, 1.0),
            BatteryLevel::High
        );
        assert_eq!(
            battery_level_of(State::Discharging, 0.75),
            BatteryLevel::High
        );
        assert_eq!(
            battery_level_of(State::Discharging, 0.51),
            BatteryLevel::High
        );
        assert_eq!(battery_level_of(State::Discharging, 0.5), BatteryLevel::Low);
    }

    /// **A discharging battery is low above 15 %.** Between 15 % (excluded)
    /// and 50 % (included) it is `Low`.
    #[test]
    fn a_discharging_battery_is_low_above_fifteen_percent() {
        assert_eq!(battery_level_of(State::Discharging, 0.5), BatteryLevel::Low);
        assert_eq!(battery_level_of(State::Discharging, 0.3), BatteryLevel::Low);
        assert_eq!(
            battery_level_of(State::Discharging, 0.16),
            BatteryLevel::Low
        );
    }

    /// **A discharging battery is critical at or under 15 %.** 15 % itself is
    /// critical, and so is every charge below it, down to an empty battery.
    #[test]
    fn a_discharging_battery_is_critical_at_or_under_fifteen_percent() {
        assert_eq!(
            battery_level_of(State::Discharging, 0.15),
            BatteryLevel::Critical
        );
        assert_eq!(
            battery_level_of(State::Discharging, 0.1),
            BatteryLevel::Critical
        );
        assert_eq!(
            battery_level_of(State::Discharging, 0.0),
            BatteryLevel::Critical
        );
    }

    /// **An empty battery is critical.** A battery the controller reports
    /// empty is not on mains (it would be charging): it is `Critical`.
    #[test]
    fn an_empty_battery_is_critical() {
        assert_eq!(battery_level_of(State::Empty, 0.0), BatteryLevel::Critical);
    }

    /// **The sysinfo monitor is no telemetry monitor.** The sensors cost
    /// milliseconds to read, and the telemetry service runs inside the frame:
    /// the monitor is read by the DCC, on its own thread, as a
    /// [`HardwareMonitor`], never registered as a [`ResourceMonitor`].
    #[test]
    fn the_sysinfo_monitor_is_no_telemetry_monitor() {
        use khora_core::telemetry::monitoring::ResourceMonitor;
        use std::marker::PhantomData;

        // Method resolution picks the first trait that applies: the one over
        // `Probe<T>` when `T` is a resource monitor, else the one over
        // `&Probe<T>`. One of the two is never picked, so never used.
        struct Probe<T>(PhantomData<T>);
        #[allow(dead_code)]
        trait Monitors {
            fn is_resource_monitor(&self) -> bool {
                true
            }
        }
        impl<T: ResourceMonitor> Monitors for Probe<T> {}
        #[allow(dead_code)]
        trait DoesNotMonitor {
            fn is_resource_monitor(&self) -> bool {
                false
            }
        }
        impl<T> DoesNotMonitor for &Probe<T> {}

        assert!(
            !(&Probe::<SysinfoMonitor>(PhantomData)).is_resource_monitor(),
            "the sysinfo monitor is not a ResourceMonitor"
        );
    }

    /// **A refreshed sysinfo monitor reads the CPU's load as a fraction.**
    /// Refreshed as a [`HardwareMonitor`] — as the DCC refreshes it — its CPU
    /// load is in `[0, 1]`, and still is after a second refresh, once the
    /// load is measured between two of them.
    #[test]
    fn a_refreshed_sysinfo_monitor_reads_the_cpu_load_as_a_fraction() {
        let monitor = SysinfoMonitor::new();
        let monitor: &dyn HardwareMonitor = &monitor;

        monitor.refresh();
        let load = monitor.cpu_load();
        assert!(
            (0.0..=1.0).contains(&load),
            "a CPU load is a fraction, got {load}"
        );

        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        monitor.refresh();
        let load = monitor.cpu_load();
        assert!(
            (0.0..=1.0).contains(&load),
            "a CPU load is a fraction after a second refresh, got {load}"
        );
    }
}
