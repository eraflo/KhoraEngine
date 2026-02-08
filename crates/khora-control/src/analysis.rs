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

//! Heuristic analysis for the DCC.
//!
//! The `HeuristicEngine` is the analytical core that evaluates the full
//! situational model (hardware state, execution phase, metric trends) to
//! decide whether a GORNA renegotiation is necessary and what the global
//! performance target should be.

use crate::context::{Context, ExecutionPhase};
use crate::metrics::MetricStore;
use khora_core::platform::{BatteryLevel, ThermalStatus};
use khora_core::telemetry::MetricId;

/// Threshold (ms) above which frame time is considered problematic.
const FRAME_TIME_WARN_THRESHOLD_MS: f32 = 18.0;
/// Threshold (ms) above which frame time is critically high.
const FRAME_TIME_CRITICAL_THRESHOLD_MS: f32 = 25.0;
/// Threshold for frame time variance indicating stutter.
const FRAME_TIME_VARIANCE_THRESHOLD: f32 = 4.0;
/// Rising trend threshold (ms per sample window) triggering preemptive action.
const FRAME_TIME_TREND_THRESHOLD: f32 = 2.0;
/// CPU load threshold for triggering negotiation.
const CPU_LOAD_CRITICAL: f32 = 0.95;
/// GPU load threshold for triggering negotiation.
const GPU_LOAD_CRITICAL: f32 = 0.95;
/// GPU load threshold for a warning-level response.
const GPU_LOAD_WARN: f32 = 0.90;

/// Analysis results and alerts produced by the `HeuristicEngine`.
#[derive(Debug, Clone)]
pub struct AnalysisReport {
    /// `true` if a resource conflict or performance drop is detected and GORNA
    /// should run a full negotiation round.
    pub needs_negotiation: bool,
    /// Suggested global target latency (in ms) derived from analysis.
    pub suggested_latency_ms: f32,
    /// `true` if the engine is in a "death spiral" — multiple subsystems are
    /// simultaneously failing to meet budgets and an emergency stop is required.
    pub death_spiral_detected: bool,
    /// Human-readable summary of analysis findings for telemetry/logging.
    pub alerts: Vec<String>,
}

impl Default for AnalysisReport {
    fn default() -> Self {
        Self {
            needs_negotiation: false,
            suggested_latency_ms: 16.66,
            death_spiral_detected: false,
            alerts: Vec::new(),
        }
    }
}

/// Analyzes metrics and context to determine engine-wide strategy changes.
pub struct HeuristicEngine;

impl HeuristicEngine {
    /// Analyzes the current situational model.
    ///
    /// Evaluates the full set of heuristics:
    /// 1. **Phase heuristics**: Adjust target FPS for the current execution phase.
    /// 2. **Thermal analysis**: Detect throttling / critical and reduce budgets.
    /// 3. **Battery analysis**: Conserve power on low/critical battery.
    /// 4. **Frame time analysis**: Detect sustained performance drops.
    /// 5. **Stutter analysis**: Detect high frame time variance.
    /// 6. **Trend analysis**: Preempt worsening performance via slope detection.
    /// 7. **CPU/GPU pressure**: Detect resource saturation.
    pub fn analyze(&self, context: &Context, store: &MetricStore) -> AnalysisReport {
        let mut report = AnalysisReport::default();
        let mut pressure_count: u32 = 0;

        // ── 1. Phase-Based Target ────────────────────────────────────────
        report.suggested_latency_ms = match context.phase {
            ExecutionPhase::Boot => 33.33, // Loading — no frame budget needed
            ExecutionPhase::Menu => 33.33, // 30 FPS in menus is sufficient
            ExecutionPhase::Simulation => 16.66, // 60 FPS target
            ExecutionPhase::Background => 200.0, // 5 FPS — absolute minimum
        };

        // Background phase always triggers negotiation so agents can throttle down.
        if context.phase == ExecutionPhase::Background {
            report.needs_negotiation = true;
            report
                .alerts
                .push("Phase: Background — reducing all agents to minimum.".into());
            return report;
        }

        // ── 2. Thermal Analysis ──────────────────────────────────────────
        match context.hardware.thermal {
            ThermalStatus::Critical => {
                log::warn!("Heuristic: CRITICAL thermal state — emergency budget reduction.");
                report.needs_negotiation = true;
                report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 50.0); // ~20 FPS cap
                report
                    .alerts
                    .push("Thermal: CRITICAL — emergency load reduction.".into());
                pressure_count += 1;
            }
            ThermalStatus::Throttling => {
                log::warn!("Heuristic: Device is throttling. Recommending load reduction.");
                report.needs_negotiation = true;
                report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 33.33); // 30 FPS cap
                report
                    .alerts
                    .push("Thermal: Throttling — capping to 30 FPS.".into());
                pressure_count += 1;
            }
            ThermalStatus::Warm => {
                log::debug!("Heuristic: Device is warm. Monitoring.");
            }
            ThermalStatus::Cool => {}
        }

        // ── 3. Battery Analysis ──────────────────────────────────────────
        match context.hardware.battery {
            BatteryLevel::Critical => {
                log::warn!("Heuristic: Battery CRITICAL — mandatory power saving.");
                report.needs_negotiation = true;
                report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 50.0); // ~20 FPS
                report
                    .alerts
                    .push("Battery: CRITICAL — mandatory power saving.".into());
                pressure_count += 1;
            }
            BatteryLevel::Low => {
                log::info!("Heuristic: Battery low — reducing target to 30 FPS.");
                report.needs_negotiation = true;
                report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 33.33);
                report
                    .alerts
                    .push("Battery: Low — capping to 30 FPS.".into());
            }
            BatteryLevel::High | BatteryLevel::Mains => {}
        }

        // ── 4. Frame Time Analysis ───────────────────────────────────────
        let frame_time_id = MetricId::new("renderer", "frame_time");
        let avg_frame_time = store.get_average(&frame_time_id);
        let has_enough_samples = store.get_sample_count(&frame_time_id) >= 10;

        if has_enough_samples {
            if avg_frame_time > FRAME_TIME_CRITICAL_THRESHOLD_MS {
                log::warn!(
                    "Heuristic: Frame time critically high ({:.2}ms). Forcing negotiation.",
                    avg_frame_time
                );
                report.needs_negotiation = true;
                report.alerts.push(format!(
                    "FrameTime: CRITICAL — avg {:.2}ms exceeds {:.0}ms.",
                    avg_frame_time, FRAME_TIME_CRITICAL_THRESHOLD_MS
                ));
                pressure_count += 1;
            } else if avg_frame_time > FRAME_TIME_WARN_THRESHOLD_MS {
                log::debug!(
                    "Heuristic: Frame time elevated ({:.2}ms). Triggering negotiation.",
                    avg_frame_time
                );
                report.needs_negotiation = true;
                report.alerts.push(format!(
                    "FrameTime: Elevated — avg {:.2}ms above {:.0}ms threshold.",
                    avg_frame_time, FRAME_TIME_WARN_THRESHOLD_MS
                ));
            }

            // ── 5. Stutter Detection (variance) ─────────────────────────
            let variance = store.get_variance(&frame_time_id);
            if variance > FRAME_TIME_VARIANCE_THRESHOLD {
                log::info!(
                    "Heuristic: High frame time variance ({:.2}). Stutter detected.",
                    variance
                );
                report.needs_negotiation = true;
                report.alerts.push(format!(
                    "Stutter: Variance {:.2} exceeds threshold {:.1}.",
                    variance, FRAME_TIME_VARIANCE_THRESHOLD
                ));
            }

            // ── 6. Trend Analysis (preemptive) ──────────────────────────
            let trend = store.get_trend(&frame_time_id);
            if trend > FRAME_TIME_TREND_THRESHOLD {
                log::info!(
                    "Heuristic: Frame time rising ({:+.2}ms trend). Preemptive negotiation.",
                    trend
                );
                report.needs_negotiation = true;
                report.alerts.push(format!(
                    "Trend: Frame time rising at {:+.2}ms/window.",
                    trend
                ));
            }
        }

        // ── 7. CPU Pressure ──────────────────────────────────────────────
        if context.hardware.cpu_load > CPU_LOAD_CRITICAL {
            log::warn!(
                "Heuristic: CPU load critical ({:.2}). Triggering negotiation.",
                context.hardware.cpu_load
            );
            report.needs_negotiation = true;
            report.alerts.push(format!(
                "CPU: Load {:.0}% exceeds critical threshold.",
                context.hardware.cpu_load * 100.0
            ));
            pressure_count += 1;
        }

        // ── 8. GPU Pressure ──────────────────────────────────────────────
        if context.hardware.gpu_load > GPU_LOAD_CRITICAL {
            log::warn!(
                "Heuristic: GPU load critical ({:.2}). Triggering negotiation.",
                context.hardware.gpu_load
            );
            report.needs_negotiation = true;
            report.alerts.push(format!(
                "GPU: Load {:.0}% exceeds critical threshold.",
                context.hardware.gpu_load * 100.0
            ));
            pressure_count += 1;
        } else if context.hardware.gpu_load > GPU_LOAD_WARN {
            log::debug!(
                "Heuristic: GPU load elevated ({:.2}).",
                context.hardware.gpu_load
            );
            report.needs_negotiation = true;
            report.alerts.push(format!(
                "GPU: Load {:.0}% above warning threshold.",
                context.hardware.gpu_load * 100.0
            ));
        }

        // ── 9. Death Spiral Detection ────────────────────────────────────
        // If 3+ independent pressure sources are active simultaneously,
        // the engine is likely in a cascading failure ("death spiral").
        if pressure_count >= 3 {
            log::error!(
                "Heuristic: DEATH SPIRAL detected ({} simultaneous pressure sources). \
                 Emergency stop required.",
                pressure_count
            );
            report.death_spiral_detected = true;
            report.needs_negotiation = true;
            report.alerts.push(format!(
                "DEATH SPIRAL: {} simultaneous pressures.",
                pressure_count
            ));
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricStore;

    fn default_context() -> Context {
        Context::default()
    }

    fn simulation_context() -> Context {
        Context {
            phase: ExecutionPhase::Simulation,
            ..Default::default()
        }
    }

    // ── Phase Heuristics ─────────────────────────────────────────────

    #[test]
    fn test_normal_simulation_no_negotiation() {
        let engine = HeuristicEngine;
        let ctx = simulation_context();
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(!report.needs_negotiation);
        assert!((report.suggested_latency_ms - 16.66).abs() < 0.1);
        assert!(!report.death_spiral_detected);
    }

    #[test]
    fn test_background_phase_triggers_negotiation() {
        let engine = HeuristicEngine;
        let mut ctx = default_context();
        ctx.phase = ExecutionPhase::Background;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.suggested_latency_ms >= 200.0);
    }

    #[test]
    fn test_menu_phase_targets_30fps() {
        let engine = HeuristicEngine;
        let mut ctx = default_context();
        ctx.phase = ExecutionPhase::Menu;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!((report.suggested_latency_ms - 33.33).abs() < 0.1);
    }

    // ── Thermal Heuristics ───────────────────────────────────────────

    #[test]
    fn test_thermal_throttling_triggers_negotiation() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.thermal = ThermalStatus::Throttling;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.suggested_latency_ms >= 33.33);
    }

    #[test]
    fn test_thermal_critical_emergency() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.thermal = ThermalStatus::Critical;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.suggested_latency_ms >= 50.0);
    }

    // ── Battery Heuristics ───────────────────────────────────────────

    #[test]
    fn test_battery_low_caps_fps() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.battery = BatteryLevel::Low;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.suggested_latency_ms >= 33.33);
    }

    #[test]
    fn test_battery_critical_aggressive_cap() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.battery = BatteryLevel::Critical;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.suggested_latency_ms >= 50.0);
    }

    // ── Frame Time Heuristics ────────────────────────────────────────

    #[test]
    fn test_high_frame_time_triggers_negotiation() {
        let engine = HeuristicEngine;
        let ctx = simulation_context();
        let mut store = MetricStore::new();

        let id = MetricId::new("renderer", "frame_time");
        for _ in 0..20 {
            store.push(id.clone(), 22.0); // 22ms > 18ms warn threshold
        }

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
    }

    #[test]
    fn test_critical_frame_time_pressure() {
        let engine = HeuristicEngine;
        let ctx = simulation_context();
        let mut store = MetricStore::new();

        let id = MetricId::new("renderer", "frame_time");
        for _ in 0..20 {
            store.push(id.clone(), 30.0); // 30ms > 25ms critical threshold
        }

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(!report.alerts.is_empty());
    }

    // ── Stutter Detection ────────────────────────────────────────────

    #[test]
    fn test_high_variance_stutter_detection() {
        let engine = HeuristicEngine;
        let ctx = simulation_context();
        let mut store = MetricStore::new();

        let id = MetricId::new("renderer", "frame_time");
        // Alternating between 5ms and 30ms = extreme stutter
        for i in 0..20 {
            store.push(id.clone(), if i % 2 == 0 { 5.0 } else { 30.0 });
        }

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.alerts.iter().any(|a| a.contains("Variance")));
    }

    // ── GPU Pressure ─────────────────────────────────────────────────

    #[test]
    fn test_gpu_pressure_triggers_negotiation() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.gpu_load = 0.96;
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.needs_negotiation);
        assert!(report.alerts.iter().any(|a| a.contains("GPU")));
    }

    // ── Death Spiral ─────────────────────────────────────────────────

    #[test]
    fn test_death_spiral_detection() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.thermal = ThermalStatus::Critical; // +1 pressure
        ctx.hardware.cpu_load = 0.98; // +1 pressure
        ctx.hardware.gpu_load = 0.97; // +1 pressure
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(report.death_spiral_detected);
        assert!(report.needs_negotiation);
        assert!(report.alerts.iter().any(|a| a.contains("DEATH SPIRAL")));
    }

    #[test]
    fn test_no_death_spiral_with_single_pressure() {
        let engine = HeuristicEngine;
        let mut ctx = simulation_context();
        ctx.hardware.thermal = ThermalStatus::Throttling; // Only 1 pressure
        let store = MetricStore::new();

        let report = engine.analyze(&ctx, &store);
        assert!(!report.death_spiral_detected);
    }
}
