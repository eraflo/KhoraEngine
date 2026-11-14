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
    /// Available VRAM in bytes (if known).
    pub available_vram: Option<u64>,
    /// Total VRAM in bytes (if known).
    pub total_vram: Option<u64>,
    /// Currently-allocated system RAM in bytes, from the tracking allocator.
    ///
    /// `None` unless a `MemoryMonitor` feeds telemetry.
    pub current_ram_bytes: Option<u64>,
    /// Developer-set system-RAM budget in bytes; `None` disables the signal.
    ///
    /// With `current_ram_bytes`, it gives
    /// [`memory_pressure`](DccContext::memory_pressure).
    pub memory_budget_bytes: Option<u64>,
}

/// The complete context model used for strategic decision making.
#[derive(Debug, Clone)]
pub struct DccContext {
    /// Observed hardware state.
    pub hardware: HardwareState,
    /// The families off in the state in force; their agents are neither asked nor lent.
    ///
    /// Set by the last
    /// [`ActivationChanged`](khora_core::telemetry::TelemetryEvent::ActivationChanged).
    pub inactive: std::collections::BTreeSet<khora_core::agent::gorna::AgentId>,
    /// The multiplier on the frame target GORNA shares, from 0.0 (emergency) to 1.0.
    ///
    /// Driven by the frame governor ([`crate::governor`]) and capped by
    /// [`safety_ceiling`]. See the book: concepts/gorna.md, "The cost model and
    /// the frame governor".
    pub global_budget_multiplier: f32,
    /// System-memory pressure in `[0, 1]`: `current_ram_bytes / memory_budget_bytes`.
    ///
    /// `0.0` when the budget is unset. Read by the heuristics and by
    /// [`safety_ceiling`].
    pub memory_pressure: f32,
}

impl Default for DccContext {
    fn default() -> Self {
        Self {
            hardware: HardwareState::default(),
            inactive: std::collections::BTreeSet::new(),
            global_budget_multiplier: 1.0,
            memory_pressure: 0.0,
        }
    }
}

/// Memory pressure at or above which the DCC clamps the budget multiplier to a
/// hard safety ceiling, regardless of where the frame governor currently sits.
pub const MEMORY_PRESSURE_CRITICAL: f32 = 0.95;

impl DccContext {
    /// Recomputes [`DccContext::memory_pressure`] from the RAM in use and the budget.
    ///
    /// An unset budget yields `0.0`. The pressure reaches the multiplier only
    /// through [`safety_ceiling`].
    pub fn refresh_memory_pressure(&mut self) {
        self.memory_pressure = match (
            self.hardware.current_ram_bytes,
            self.hardware.memory_budget_bytes,
        ) {
            (Some(current), Some(budget)) if budget > 0 => {
                (current as f32 / budget as f32).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
    }
}

/// The hard ceiling on the budget multiplier for the current context.
///
/// It only ever lowers the multiplier, at once, on `Critical` thermal or
/// battery or near-budget memory, without waiting for the governor.
pub fn safety_ceiling(ctx: &DccContext) -> f32 {
    let mut ceiling = 1.0_f32;
    if ctx.hardware.thermal == ThermalStatus::Critical {
        ceiling = ceiling.min(0.4);
    }
    if ctx.hardware.battery == BatteryLevel::Critical {
        ceiling = ceiling.min(0.5);
    }
    if ctx.memory_pressure >= MEMORY_PRESSURE_CRITICAL {
        ceiling = ceiling.min(0.5);
    }
    ceiling
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context_full_budget() {
        let ctx = DccContext::default();
        assert_eq!(ctx.global_budget_multiplier, 1.0);
        assert!(ctx.inactive.is_empty(), "every family on");
    }

    #[test]
    fn test_memory_pressure_from_budget() {
        let mut ctx = DccContext::default();
        ctx.hardware.current_ram_bytes = Some(960);
        ctx.hardware.memory_budget_bytes = Some(1000);
        ctx.refresh_memory_pressure();
        assert!((ctx.memory_pressure - 0.96).abs() < 0.001);
    }

    #[test]
    fn test_memory_pressure_zero_without_budget() {
        let mut ctx = DccContext::default();
        ctx.hardware.current_ram_bytes = Some(10_000_000);
        ctx.hardware.memory_budget_bytes = None;
        ctx.refresh_memory_pressure();
        assert_eq!(ctx.memory_pressure, 0.0);
    }

    #[test]
    fn test_safety_ceiling_open_when_healthy() {
        let ctx = DccContext::default();
        assert_eq!(safety_ceiling(&ctx), 1.0);
    }

    #[test]
    fn test_safety_ceiling_critical_thermal() {
        let mut ctx = DccContext::default();
        ctx.hardware.thermal = ThermalStatus::Critical;
        assert!((safety_ceiling(&ctx) - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_critical_battery() {
        let mut ctx = DccContext::default();
        ctx.hardware.battery = BatteryLevel::Critical;
        assert!((safety_ceiling(&ctx) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_high_memory_pressure() {
        let ctx = DccContext {
            memory_pressure: 0.96,
            ..Default::default()
        };
        assert!((safety_ceiling(&ctx) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_takes_minimum() {
        let mut ctx = DccContext::default();
        ctx.hardware.thermal = ThermalStatus::Critical; // 0.4
        ctx.hardware.battery = BatteryLevel::Critical; // 0.5
                                                       // Most restrictive wins.
        assert!((safety_ceiling(&ctx) - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_non_critical_states_leave_ceiling_open() {
        let mut ctx = DccContext::default();
        ctx.hardware.thermal = ThermalStatus::Throttling;
        ctx.hardware.battery = BatteryLevel::Low;
        // Throttling and Low shape the setpoint, not a hard clamp.
        assert_eq!(safety_ceiling(&ctx), 1.0);
    }
}
