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

//! The DCC's heuristics: whether to renegotiate, and what frame time to aim for.
//!
//! See the book: concepts/gorna.md, "The heuristics".

use crate::dcc_context::DccContext;
use crate::metrics::MetricStore;
use khora_core::platform::{BatteryLevel, ThermalStatus};
use khora_core::telemetry::MetricId;

mod report;

pub use report::AnalysisReport;

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
/// Memory-pressure threshold (fraction of the RAM budget) for a critical response.
const MEM_PRESSURE_CRITICAL: f32 = 0.95;
/// Memory-pressure threshold for a warning-level response.
const MEM_PRESSURE_WARN: f32 = 0.85;
/// Coefficient of variation (stddev/mean) of resident bytes above which memory churns.
///
/// An alert only, never a control action, so a noisy estimate can't misfire.
const MEM_CHURN_COV_THRESHOLD: f32 = 0.15;

/// Analyzes metrics and context to determine engine-wide strategy changes.
pub struct HeuristicEngine;

impl HeuristicEngine {
    /// Runs every heuristic on the context and the metrics, and reports what they conclude.
    pub fn analyze(&self, context: &DccContext, store: &MetricStore) -> AnalysisReport {
        // ── Target latency (60 FPS baseline) ──────────────────────────
        let mut report = AnalysisReport {
            suggested_latency_ms: 16.66,
            ..AnalysisReport::default()
        };

        let mut pressure_count = analyze_thermal(context, &mut report);
        pressure_count += analyze_battery(context, &mut report);
        pressure_count += analyze_frame_time(store, &mut report);
        pressure_count += analyze_cpu(context, &mut report);
        pressure_count += analyze_gpu(context, &mut report);
        pressure_count += analyze_memory(context, &mut report);
        flag_allocation_churn(store, &mut report);
        flag_death_spiral(pressure_count, &mut report);

        report
    }
}

/// Reports the thermal state; returns the pressure sources it counted.
fn analyze_thermal(context: &DccContext, report: &mut AnalysisReport) -> u32 {
    match context.hardware.thermal {
        ThermalStatus::Critical => {
            log::warn!("Heuristic: CRITICAL thermal state — emergency budget reduction.");
            report.needs_negotiation = true;
            report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 50.0); // ~20 FPS cap
            report
                .alerts
                .push("Thermal: CRITICAL — emergency load reduction.".into());
            1
        }
        ThermalStatus::Throttling => {
            log::warn!("Heuristic: Device is throttling. Recommending load reduction.");
            report.needs_negotiation = true;
            report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 33.33); // 30 FPS cap
            report
                .alerts
                .push("Thermal: Throttling — capping to 30 FPS.".into());
            1
        }
        ThermalStatus::Warm => {
            log::debug!("Heuristic: Device is warm. Monitoring.");
            0
        }
        ThermalStatus::Cool => 0,
    }
}

/// Reports the battery level; returns the pressure sources it counted.
fn analyze_battery(context: &DccContext, report: &mut AnalysisReport) -> u32 {
    match context.hardware.battery {
        BatteryLevel::Critical => {
            log::warn!("Heuristic: Battery CRITICAL — mandatory power saving.");
            report.needs_negotiation = true;
            report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 50.0); // ~20 FPS
            report
                .alerts
                .push("Battery: CRITICAL — mandatory power saving.".into());
            1
        }
        BatteryLevel::Low => {
            log::info!("Heuristic: Battery low — reducing target to 30 FPS.");
            report.needs_negotiation = true;
            report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 33.33);
            report
                .alerts
                .push("Battery: Low — capping to 30 FPS.".into());
            0
        }
        BatteryLevel::High | BatteryLevel::Mains => 0,
    }
}

/// Reports the frame time, its stutter and its trend; returns the pressure
/// sources it counted.
fn analyze_frame_time(store: &MetricStore, report: &mut AnalysisReport) -> u32 {
    // The frame's critical path, from the frame reports.
    let frame_time_id = MetricId::new("frame", "critical_ms");
    let avg_frame_time = store.get_average(&frame_time_id);
    let has_enough_samples = store.get_sample_count(&frame_time_id) >= 10;
    if !has_enough_samples {
        return 0;
    }
    let mut pressure_count = 0;

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

    // ── Stutter Detection (variance) ─────────────────────────
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

    // ── Trend Analysis (preemptive) ──────────────────────────
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
    pressure_count
}

/// Reports the CPU load; returns the pressure sources it counted.
fn analyze_cpu(context: &DccContext, report: &mut AnalysisReport) -> u32 {
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
        1
    } else {
        0
    }
}

/// Reports the GPU load; returns the pressure sources it counted.
fn analyze_gpu(context: &DccContext, report: &mut AnalysisReport) -> u32 {
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
        1
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
        0
    } else {
        0
    }
}

/// Reports the memory pressure; returns the pressure sources it counted.
///
/// Near the RAM budget, downgrade so memory-heavy rows aren't chosen. Inert
/// when no budget is set (pressure 0).
fn analyze_memory(context: &DccContext, report: &mut AnalysisReport) -> u32 {
    if context.memory_pressure > MEM_PRESSURE_CRITICAL {
        log::warn!(
            "Heuristic: Memory pressure critical ({:.0}%). Triggering negotiation.",
            context.memory_pressure * 100.0
        );
        report.needs_negotiation = true;
        report.alerts.push(format!(
            "Memory: pressure {:.0}% exceeds critical threshold.",
            context.memory_pressure * 100.0
        ));
        1
    } else if context.memory_pressure > MEM_PRESSURE_WARN {
        log::debug!(
            "Heuristic: Memory pressure elevated ({:.0}%).",
            context.memory_pressure * 100.0
        );
        report.needs_negotiation = true;
        report.alerts.push(format!(
            "Memory: pressure {:.0}% above warning threshold.",
            context.memory_pressure * 100.0
        ));
        0
    } else {
        0
    }
}

/// Alerts on a high volatility of resident bytes, never forcing a strategy change.
///
/// It points to per-frame allocation hotspots (allocator locks, page faults:
/// a hitch risk).
fn flag_allocation_churn(store: &MetricStore, report: &mut AnalysisReport) {
    let mem_bytes_id = MetricId::new("memory", "current_bytes");
    if store.get_sample_count(&mem_bytes_id) >= 10 {
        let avg = store.get_average(&mem_bytes_id);
        if avg > 0.0 {
            let cov = store.get_variance(&mem_bytes_id).sqrt() / avg;
            if cov > MEM_CHURN_COV_THRESHOLD {
                log::info!("Heuristic: high allocation churn (CoV {:.2}).", cov);
                report.alerts.push(format!(
                    "Memory: high allocation churn (CoV {:.2}) — possible per-frame alloc hotspot.",
                    cov
                ));
            }
        }
    }
}

/// Flags a death spiral: three or more independent pressure sources at once,
/// a cascading failure.
fn flag_death_spiral(pressure_count: u32, report: &mut AnalysisReport) {
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
}

#[cfg(test)]
mod tests;
