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

use super::*;
use crate::dcc_context::HardwareState;
use crate::metrics::MetricStore;

fn default_context() -> DccContext {
    DccContext::default()
}

fn simulation_context() -> DccContext {
    DccContext::default()
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
fn test_default_latency_target() {
    let engine = HeuristicEngine;
    let ctx = default_context();
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!((report.suggested_latency_ms - 16.66).abs() < 0.1);
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

    let id = MetricId::new("frame", "critical_ms");
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

    let id = MetricId::new("frame", "critical_ms");
    for _ in 0..20 {
        store.push(id.clone(), 30.0); // 30ms > 25ms critical threshold
    }

    let report = engine.analyze(&ctx, &store);
    assert!(report.needs_negotiation);
    assert!(!report.alerts.is_empty());
}

/// **Ten frames over the critical threshold raise a critical alert.**
///
/// The heuristics read the frame's critical path, `frame:critical_ms`: ten
/// samples at 30 ms, over the 25 ms threshold, ask for a negotiation and say
/// why.
#[test]
fn ten_critical_path_samples_over_the_threshold_raise_a_critical_alert() {
    let engine = HeuristicEngine;
    let ctx = simulation_context();
    let mut store = MetricStore::new();

    let id = MetricId::new("frame", "critical_ms");
    for _ in 0..10 {
        store.push(id.clone(), 30.0);
    }

    let report = engine.analyze(&ctx, &store);
    assert!(report.needs_negotiation);
    assert!(
        report.alerts.iter().any(|a| a.contains("CRITICAL")),
        "{:?}",
        report.alerts
    );
}

/// **The renderer's frame-time metric does not reach the heuristics.**
///
/// The same ten samples at 30 ms, on `renderer:frame_time`, ask for nothing:
/// the frame's time comes from its report, not from that metric.
#[test]
fn the_renderer_frame_time_metric_raises_nothing() {
    let engine = HeuristicEngine;
    let ctx = simulation_context();
    let mut store = MetricStore::new();

    let id = MetricId::new("renderer", "frame_time");
    for _ in 0..10 {
        store.push(id.clone(), 30.0);
    }

    let report = engine.analyze(&ctx, &store);
    assert!(!report.needs_negotiation);
    assert!(report.alerts.is_empty(), "{:?}", report.alerts);
}

// ── Stutter Detection ────────────────────────────────────────────

#[test]
fn test_high_variance_stutter_detection() {
    let engine = HeuristicEngine;
    let ctx = simulation_context();
    let mut store = MetricStore::new();

    let id = MetricId::new("frame", "critical_ms");
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
    ctx.hardware.gpu_load = Some(0.96);
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!(report.needs_negotiation);
    assert!(report.alerts.iter().any(|a| a.contains("GPU")));
}

/// **An unknown GPU load raises nothing, and counts as no pressure.**
///
/// Critical heat and a saturated processor are two pressures; a GPU whose
/// load nobody measured is not a third, so no death spiral.
#[test]
fn an_unknown_gpu_load_raises_nothing() {
    let engine = HeuristicEngine;
    let mut ctx = simulation_context();
    ctx.hardware.thermal = ThermalStatus::Critical;
    ctx.hardware.cpu_load = 0.98;
    ctx.hardware.gpu_load = None;
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!(!report.alerts.iter().any(|a| a.contains("GPU")));
    assert!(!report.death_spiral_detected, "two pressures, not three");
}

/// `count` frames that kept the GPU busy `gpu_ms` each, as the DCC files
/// their frame reports.
fn gpu_frames(count: usize, gpu_ms: f32) -> MetricStore {
    let mut store = MetricStore::new();
    for _ in 0..count {
        store.push(MetricId::new("frame", "gpu_ms"), gpu_ms);
    }
    store
}

/// **The GPU load is the frames' GPU time over the frame time aimed for.**
///
/// 16 ms of GPU work against the 16.66 ms setpoint is a load of 0.96; the
/// same work under a throttled device, aiming for 33.33 ms, is 0.48.
#[test]
fn the_gpu_load_is_the_frames_gpu_time_over_the_setpoint() {
    let store = gpu_frames(10, 16.0);
    let mut hardware = HardwareState::default();

    let full = gpu_load(&store, &hardware).expect("the frames were timed on the GPU");
    hardware.thermal = ThermalStatus::Throttling;
    let throttled = gpu_load(&store, &hardware).expect("the frames were timed on the GPU");

    assert!((full - 16.0 / 16.66).abs() < 0.01, "{full}");
    assert!((throttled - 16.0 / 33.33).abs() < 0.01, "{throttled}");
}

/// **A GPU over the frame time aimed for is loaded fully, not more.**
#[test]
fn a_gpu_over_the_setpoint_is_loaded_fully() {
    let store = gpu_frames(10, 40.0);

    assert_eq!(gpu_load(&store, &HardwareState::default()), Some(1.0));
}

/// **No frame timed on the GPU leaves its load unknown, not idle.**
#[test]
fn no_frame_timed_on_the_gpu_leaves_its_load_unknown() {
    assert_eq!(
        gpu_load(&MetricStore::new(), &HardwareState::default()),
        None
    );
}

/// **The setpoint is 60 frames a second, relaxed by heat or a low battery.**
#[test]
fn the_setpoint_is_relaxed_by_heat_or_a_low_battery() {
    let mut hardware = HardwareState::default();
    assert!((setpoint_ms(&hardware) - 16.66).abs() < 0.01);

    hardware.battery = BatteryLevel::Low;
    assert!((setpoint_ms(&hardware) - 33.33).abs() < 0.01);

    hardware.thermal = ThermalStatus::Critical;
    assert!((setpoint_ms(&hardware) - 50.0).abs() < 0.01);
}

// ── Memory Heuristics ────────────────────────────────────────────

#[test]
fn test_memory_pressure_triggers_negotiation() {
    let engine = HeuristicEngine;
    let mut ctx = simulation_context();
    ctx.memory_pressure = 0.96; // above critical
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!(report.needs_negotiation);
    assert!(report.alerts.iter().any(|a| a.contains("Memory")));
}

#[test]
fn test_no_memory_pressure_no_alert() {
    let engine = HeuristicEngine;
    let ctx = simulation_context(); // memory_pressure defaults to 0.0
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!(!report.alerts.iter().any(|a| a.contains("Memory")));
}

// ── Death Spiral ─────────────────────────────────────────────────

#[test]
fn test_death_spiral_detection() {
    let engine = HeuristicEngine;
    let mut ctx = simulation_context();
    ctx.hardware.thermal = ThermalStatus::Critical; // +1 pressure
    ctx.hardware.cpu_load = 0.98; // +1 pressure
    ctx.hardware.gpu_load = Some(0.97); // +1 pressure
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
