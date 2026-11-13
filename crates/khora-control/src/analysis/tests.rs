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

/// **Ten frames over the critical threshold raise a critical alert.** The
/// heuristics read the frame's critical path, `frame:critical_ms`: ten
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

/// **The renderer's frame-time metric no longer reaches the heuristics.**
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
    ctx.hardware.gpu_load = 0.96;
    let store = MetricStore::new();

    let report = engine.analyze(&ctx, &store);
    assert!(report.needs_negotiation);
    assert!(report.alerts.iter().any(|a| a.contains("GPU")));
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
