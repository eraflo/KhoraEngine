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
    /// Global budget multiplier applied to all frame budgets for graceful
    /// performance degradation. Ranges from 0.0 (emergency) to 1.0 (full
    /// performance).
    ///
    /// Driven by the DCC's frame-time **PID** controller (`khora_core::control::pid`),
    /// not a static table: the loop asservits this value so the *measured* frame
    /// time tracks the heuristic-suggested latency (`AnalysisReport::suggested_latency_ms`,
    /// itself modulated by thermal/battery/phase). On `Critical` thermal/battery or
    /// high memory pressure the DCC additionally clamps it to a hard safety ceiling.
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

/// Memory pressure at or above which the DCC clamps the budget multiplier to a
/// hard safety ceiling, regardless of where the PID loop currently sits.
pub const MEMORY_PRESSURE_CRITICAL: f32 = 0.95;

impl Context {
    /// Recomputes [`Context::memory_pressure`] from the current RAM usage versus
    /// the developer-set budget.
    ///
    /// Pressure is `current_ram_bytes / memory_budget_bytes`, clamped to `[0, 1]`.
    /// An unset budget yields `0.0` (the signal is inert). This is a first-class
    /// resource signal consumed by the heuristic engine and the AGDF repack gate;
    /// it no longer feeds the budget multiplier directly — that is the PID loop's
    /// job — but it does drive the DCC's hard safety clamp (see [`safety_ceiling`]).
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
/// The PID loop regulates the multiplier smoothly toward the frame-time target,
/// but emergencies (`Critical` thermal/battery, near-budget memory pressure)
/// demand an immediate cap that does not wait for the loop to converge. This is
/// the feedforward / safety half of the controller: it can only ever *lower* the
/// multiplier, never raise it.
pub fn safety_ceiling(ctx: &Context) -> f32 {
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
        let ctx = Context::default();
        assert_eq!(ctx.global_budget_multiplier, 1.0);
        assert_eq!(ctx.mode, EngineMode::Playing);
    }

    #[test]
    fn test_memory_pressure_from_budget() {
        let mut ctx = Context::default();
        ctx.hardware.current_ram_bytes = Some(960);
        ctx.hardware.memory_budget_bytes = Some(1000);
        ctx.refresh_memory_pressure();
        assert!((ctx.memory_pressure - 0.96).abs() < 0.001);
    }

    #[test]
    fn test_memory_pressure_zero_without_budget() {
        let mut ctx = Context::default();
        ctx.hardware.current_ram_bytes = Some(10_000_000);
        ctx.hardware.memory_budget_bytes = None;
        ctx.refresh_memory_pressure();
        assert_eq!(ctx.memory_pressure, 0.0);
    }

    #[test]
    fn test_safety_ceiling_open_when_healthy() {
        let ctx = Context::default();
        assert_eq!(safety_ceiling(&ctx), 1.0);
    }

    #[test]
    fn test_safety_ceiling_critical_thermal() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Critical;
        assert!((safety_ceiling(&ctx) - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_critical_battery() {
        let mut ctx = Context::default();
        ctx.hardware.battery = BatteryLevel::Critical;
        assert!((safety_ceiling(&ctx) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_high_memory_pressure() {
        let ctx = Context {
            memory_pressure: 0.96,
            ..Default::default()
        };
        assert!((safety_ceiling(&ctx) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_safety_ceiling_takes_minimum() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Critical; // 0.4
        ctx.hardware.battery = BatteryLevel::Critical; // 0.5
                                                       // Most restrictive wins.
        assert!((safety_ceiling(&ctx) - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_non_critical_states_leave_ceiling_open() {
        let mut ctx = Context::default();
        ctx.hardware.thermal = ThermalStatus::Throttling;
        ctx.hardware.battery = BatteryLevel::Low;
        // Throttling / Low now shape the setpoint, not a hard clamp.
        assert_eq!(safety_ceiling(&ctx), 1.0);
    }
}
