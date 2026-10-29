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

//! The device's tests: its VRAM monitor, its buffer descriptors, and how it
//! holds up when many threads call it at once.

mod buffer_descriptor;
mod concurrency;

use crate::telemetry::gpu_monitor::GpuMonitor;
use khora_core::renderer::api::device::RenderStats;
use khora_core::telemetry::{MonitoredResourceType, ResourceMonitor};

#[test]
fn gpu_monitor_creation() {
    let monitor = GpuMonitor::new("Test_WGPU".to_string());
    assert_eq!(monitor.monitor_id(), "Gpu_Test_WGPU");
    assert_eq!(monitor.resource_type(), MonitoredResourceType::Gpu);
    assert!(monitor.get_gpu_report().is_none());
}

#[test]
fn gpu_monitor_update_stats() {
    let monitor = GpuMonitor::new("Test_WGPU".to_string());

    // Create sample render stats
    let stats = RenderStats {
        frame_number: 42,
        cpu_preparation_time_ms: 1.5,
        cpu_render_submission_time_ms: 0.2,
        gpu_main_pass_time_ms: 8.0,
        gpu_frame_total_time_ms: 10.5,
        draw_calls: 100,
        triangles_rendered: 5000,
        vram_usage_estimate_mb: 256.0,
    };

    monitor.update_from_frame_stats(&stats);

    let report = monitor.get_gpu_report().unwrap();
    assert_eq!(report.frame_number, 42);
    assert_eq!(report.main_pass_duration_us(), Some(8000)); // 8ms derived
    assert_eq!(report.frame_total_duration_us(), Some(10500)); // 10.5ms derived
    assert_eq!(report.cpu_preparation_time_us, Some(1500)); // 1.5ms = 1500μs
    assert_eq!(report.cpu_submission_time_us, Some(200)); // 0.2ms = 200μs
}

#[test]
fn gpu_monitor_resource_monitor_trait() {
    let monitor = GpuMonitor::new("Test_WGPU".to_string());

    // Test ResourceMonitor trait implementation
    let usage_report = monitor.get_usage_report();
    assert_eq!(usage_report.current_bytes, 0);
    assert_eq!(usage_report.peak_bytes, None);
    assert_eq!(usage_report.total_capacity_bytes, None);

    // Performance report should be None initially
    assert!(monitor.get_gpu_report().is_none());
}
