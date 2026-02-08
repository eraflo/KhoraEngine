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

//! Context for the Dynamic Context Core.

pub use khora_core::platform::{BatteryLevel, ThermalStatus};

/// Hardware context observed by the DCC.
#[derive(Debug, Clone, Default)]
pub struct HardwareState {
    /// Current thermal status.
    pub thermal: ThermalStatus,
    /// Current battery/power status.
    pub battery: BatteryLevel,
    /// Overall CPU load (0.0 to 1.0).
    pub cpu_load: f32,
    /// Overall GPU load (0.0 to 1.0).
    pub gpu_load: f32,
}

/// The high-level workload state of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionPhase {
    /// Engine is starting up and loading assets.
    #[default]
    Boot,
    /// User is in an interactive menu.
    Menu,
    /// Full simulation is running (gameplay).
    Simulation,
    /// Application is minimized or lost focus.
    Background,
}

/// The complete context model used for strategic decision making.
#[derive(Debug, Clone)]
pub struct Context {
    /// Observed hardware state.
    pub hardware: HardwareState,
    /// Current engine execution phase.
    pub phase: ExecutionPhase,
    /// Global budget multiplier derived from thermal and battery state.
    ///
    /// Applied to all frame budgets to implement graceful performance degradation.
    /// Ranges from 0.0 (emergency) to 1.0 (full performance).
    ///
    /// | Condition | Multiplier |
    /// |---|---|
    /// | Cool + Mains | 1.0 |
    /// | Warm | 0.9 |
    /// | Battery Low | 0.8 |
    /// | Throttling | 0.6 |
    /// | Critical thermal or battery | 0.4 |
    pub global_budget_multiplier: f32,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            hardware: HardwareState::default(),
            phase: ExecutionPhase::default(),
            global_budget_multiplier: 1.0,
        }
    }
}

impl Context {
    /// Recomputes `global_budget_multiplier` from the current hardware state.
    ///
    /// This should be called whenever `hardware.thermal` or `hardware.battery` changes.
    pub fn refresh_budget_multiplier(&mut self) {
        let thermal_factor: f32 = match self.hardware.thermal {
            ThermalStatus::Cool => 1.0,
            ThermalStatus::Warm => 0.9,
            ThermalStatus::Throttling => 0.6,
            ThermalStatus::Critical => 0.4,
        };

        let battery_factor = match self.hardware.battery {
            BatteryLevel::Mains => 1.0,
            BatteryLevel::High => 1.0,
            BatteryLevel::Low => 0.8,
            BatteryLevel::Critical => 0.5,
        };

        // Take the more restrictive of the two factors.
        self.global_budget_multiplier = thermal_factor.min(battery_factor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context_full_budget() {
        let ctx = Context::default();
        assert_eq!(ctx.global_budget_multiplier, 1.0);
        assert_eq!(ctx.phase, ExecutionPhase::Boot);
    }

    #[test]
    fn test_cool_mains_full_multiplier() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Cool;
        ctx.hardware.battery = BatteryLevel::Mains;
        ctx.refresh_budget_multiplier();
        assert_eq!(ctx.global_budget_multiplier, 1.0);
    }

    #[test]
    fn test_warm_reduces_multiplier() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Warm;
        ctx.refresh_budget_multiplier();
        assert!((ctx.global_budget_multiplier - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_throttling_heavy_reduction() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Throttling;
        ctx.refresh_budget_multiplier();
        assert!((ctx.global_budget_multiplier - 0.6).abs() < 0.001);
    }

    #[test]
    fn test_critical_thermal_severe_reduction() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Critical;
        ctx.refresh_budget_multiplier();
        assert!((ctx.global_budget_multiplier - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_battery_low_reduces_multiplier() {
        let mut ctx = Context::default();
        ctx.hardware.battery = BatteryLevel::Low;
        ctx.refresh_budget_multiplier();
        assert!((ctx.global_budget_multiplier - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_battery_critical_severe_reduction() {
        let mut ctx = Context::default();
        ctx.hardware.battery = BatteryLevel::Critical;
        ctx.refresh_budget_multiplier();
        assert!((ctx.global_budget_multiplier - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_combined_thermal_and_battery_takes_minimum() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Throttling; // 0.6
        ctx.hardware.battery = BatteryLevel::Critical;     // 0.5
        ctx.refresh_budget_multiplier();
        // Should pick the more restrictive value: 0.5
        assert!((ctx.global_budget_multiplier - 0.5).abs() < 0.001);
    }
}
