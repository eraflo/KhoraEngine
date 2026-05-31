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

pub use khora_core::agent::EngineMode;
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
    /// Available VRAM in bytes (if known).
    pub available_vram: Option<u64>,
    /// Total VRAM in bytes (if known).
    pub total_vram: Option<u64>,
    /// Currently-allocated system RAM in bytes, from the tracking allocator
    /// (if a `MemoryMonitor` is feeding telemetry). `None` when unknown.
    pub current_ram_bytes: Option<u64>,
    /// Developer-set system-RAM budget in bytes. When both this and
    /// `current_ram_bytes` are known, the DCC derives `memory_pressure` and
    /// degrades budgets as the ceiling approaches. `None` disables the signal
    /// (no overhead, no effect) — it matters chiefly on memory-constrained
    /// targets (console / mobile / iGPU).
    pub memory_budget_bytes: Option<u64>,
}

/// The complete context model used for strategic decision making.
#[derive(Debug, Clone)]
pub struct Context {
    /// Observed hardware state.
    pub hardware: HardwareState,
    /// Current engine mode.
    pub mode: EngineMode,
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
    /// System-memory pressure in `[0, 1]` — `current_ram_bytes / memory_budget_bytes`,
    /// or `0.0` when the budget is unset. A first-class resource signal alongside
    /// thermal/CPU/GPU: drives graceful degradation here and feeds the AGDF
    /// repack-headroom gate (a repack transiently doubles a column, so it is
    /// declined under high pressure).
    pub memory_pressure: f32,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            hardware: HardwareState::default(),
            mode: EngineMode::Playing,
            global_budget_multiplier: 1.0,
            memory_pressure: 0.0,
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

        // Derive memory pressure from the current RAM vs the developer budget.
        // Unset budget → pressure 0 → memory has no effect on the multiplier.
        self.memory_pressure = match (
            self.hardware.current_ram_bytes,
            self.hardware.memory_budget_bytes,
        ) {
            (Some(current), Some(budget)) if budget > 0 => {
                (current as f32 / budget as f32).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let memory_factor: f32 = if self.memory_pressure >= 0.95 {
            0.5
        } else if self.memory_pressure >= 0.85 {
            0.8
        } else {
            1.0
        };

        // Take the most restrictive of the factors (thermal, battery, memory).
        self.global_budget_multiplier = thermal_factor.min(battery_factor).min(memory_factor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context_full_budget() {
        let ctx = Context::default();
        assert_eq!(ctx.global_budget_multiplier, 1.0);
        assert_eq!(ctx.mode, EngineMode::Playing);
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
    fn test_memory_pressure_reduces_multiplier() {
        let mut ctx = Context::default();
        ctx.hardware.current_ram_bytes = Some(960);
        ctx.hardware.memory_budget_bytes = Some(1000);
        ctx.refresh_budget_multiplier();
        assert!((ctx.memory_pressure - 0.96).abs() < 0.001);
        // ≥0.95 pressure → 0.5 memory factor dominates.
        assert!((ctx.global_budget_multiplier - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_memory_pressure_zero_without_budget() {
        let mut ctx = Context::default();
        ctx.hardware.current_ram_bytes = Some(10_000_000);
        ctx.hardware.memory_budget_bytes = None;
        ctx.refresh_budget_multiplier();
        assert_eq!(ctx.memory_pressure, 0.0);
        assert_eq!(ctx.global_budget_multiplier, 1.0);
    }

    #[test]
    fn test_combined_thermal_and_battery_takes_minimum() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Throttling; // 0.6
        ctx.hardware.battery = BatteryLevel::Critical; // 0.5
        ctx.refresh_budget_multiplier();
        // Should pick the more restrictive value: 0.5
        assert!((ctx.global_budget_multiplier - 0.5).abs() < 0.001);
    }
}
