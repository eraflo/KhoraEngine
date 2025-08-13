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

use std::borrow::Cow;
use std::sync::Mutex;

use crate::core::monitoring::{self as core_monitoring, ResourceMonitor};
use crate::subsystems::renderer::api::common_types::{GpuPerfHook, RenderStats};
use crate::subsystems::renderer::traits::render_system::RenderSystem;

/// GPU performance monitor that works with any RenderSystem implementation.
/// This provides a consistent ResourceMonitor interface for GPU timing metrics
/// regardless of the underlying graphics backend (WGPU, Vulkan, etc.).
#[derive(Debug)]
pub struct GpuPerformanceMonitor {
    system_name: String,
    last_frame_stats: Mutex<Option<core_monitoring::GpuPerformanceReport>>,
}

impl GpuPerformanceMonitor {
    /// Create a new GPU performance monitor
    pub fn new(system_name: String) -> Self {
        Self {
            system_name,
            last_frame_stats: Mutex::new(None),
        }
    }

    /// Update performance metrics from any RenderSystem and RenderStats
    pub fn update_from_render_system(&self, render_system: &dyn RenderSystem, stats: &RenderStats) {
        let mut report = core_monitoring::GpuPerformanceReport {
            frame_number: stats.frame_number,
            hook_timings_us: [None; 4],
            cpu_preparation_time_us: Some((stats.cpu_preparation_time_ms * 1000.0) as u32),
            cpu_submission_time_us: Some((stats.cpu_render_submission_time_ms * 1000.0) as u32),
        };

        // Collect timing for each GPU performance hook
        for hook in GpuPerfHook::ALL {
            if let Some(timing_ms) = render_system.gpu_hook_time_ms(hook) {
                report.set_hook_timing_us(hook, Some((timing_ms * 1000.0) as u32));
            }
        }

        // Store the updated report
        let mut last_stats = self.last_frame_stats.lock().unwrap();
        *last_stats = Some(report);
    }

    /// Update from render statistics with derived hook timings
    /// This is a fallback method when direct hook access isn't available
    pub fn update_from_render_stats(&self, stats: &RenderStats) {
        let mut report = core_monitoring::GpuPerformanceReport {
            frame_number: stats.frame_number,
            hook_timings_us: [None; 4],
            cpu_preparation_time_us: Some((stats.cpu_preparation_time_ms * 1000.0) as u32),
            cpu_submission_time_us: Some((stats.cpu_render_submission_time_ms * 1000.0) as u32),
        };

        // If we have GPU timing data in RenderStats, derive hook timings
        if stats.gpu_main_pass_time_ms > 0.0 {
            let main_pass_us = (stats.gpu_main_pass_time_ms * 1000.0) as u32;
            report.set_hook_timing_us(GpuPerfHook::MainPassBegin, Some(0));
            report.set_hook_timing_us(GpuPerfHook::MainPassEnd, Some(main_pass_us));
        }

        if stats.gpu_frame_total_time_ms > 0.0 {
            let frame_total_us = (stats.gpu_frame_total_time_ms * 1000.0) as u32;
            report.set_hook_timing_us(GpuPerfHook::FrameStart, Some(0));
            report.set_hook_timing_us(GpuPerfHook::FrameEnd, Some(frame_total_us));
        }

        let mut last_stats = self.last_frame_stats.lock().unwrap();
        *last_stats = Some(report);
    }
}

impl ResourceMonitor for GpuPerformanceMonitor {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Owned(format!("GpuPerf_{}", self.system_name))
    }

    fn resource_type(&self) -> core_monitoring::MonitoredResourceType {
        core_monitoring::MonitoredResourceType::GpuPerformance
    }

    fn get_usage_report(&self) -> core_monitoring::ResourceUsageReport {
        // For GPU performance monitoring, usage report is not applicable
        // We use get_gpu_performance_report instead
        core_monitoring::ResourceUsageReport::default()
    }

    fn get_gpu_performance_report(&self) -> Option<core_monitoring::GpuPerformanceReport> {
        *self.last_frame_stats.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::renderer::api::common_types::RenderStats;

    #[test]
    fn gpu_performance_monitor_creation() {
        let monitor = GpuPerformanceMonitor::new("TestBackend".to_string());
        assert_eq!(monitor.monitor_id(), "GpuPerf_TestBackend");
        assert_eq!(
            monitor.resource_type(),
            core_monitoring::MonitoredResourceType::GpuPerformance
        );
        assert!(monitor.get_gpu_performance_report().is_none());
    }

    #[test]
    fn gpu_performance_monitor_update_stats() {
        let monitor = GpuPerformanceMonitor::new("TestBackend".to_string());

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

        monitor.update_from_render_stats(&stats);

        let report = monitor.get_gpu_performance_report().unwrap();
        assert_eq!(report.frame_number, 42);
        assert_eq!(report.main_pass_duration_us(), Some(8000)); // 8ms derived
        assert_eq!(report.frame_total_duration_us(), Some(10500)); // 10.5ms derived
        assert_eq!(report.cpu_preparation_time_us, Some(1500)); // 1.5ms = 1500μs
        assert_eq!(report.cpu_submission_time_us, Some(200)); // 0.2ms = 200μs
    }

    #[test]
    fn gpu_performance_report_hook_methods() {
        let mut report = core_monitoring::GpuPerformanceReport::default();

        // Test setting and getting hook timings
        report.set_hook_timing_us(GpuPerfHook::FrameStart, Some(1000));
        report.set_hook_timing_us(GpuPerfHook::MainPassBegin, Some(2000));
        report.set_hook_timing_us(GpuPerfHook::MainPassEnd, Some(8000));
        report.set_hook_timing_us(GpuPerfHook::FrameEnd, Some(10000));

        assert_eq!(
            report.get_hook_timing_us(GpuPerfHook::FrameStart),
            Some(1000)
        );
        assert_eq!(
            report.get_hook_timing_us(GpuPerfHook::MainPassBegin),
            Some(2000)
        );
        assert_eq!(report.main_pass_duration_us(), Some(6000)); // 8000 - 2000
        assert_eq!(report.frame_total_duration_us(), Some(9000)); // 10000 - 1000
    }

    #[test]
    fn gpu_performance_report_missing_data() {
        let report = core_monitoring::GpuPerformanceReport::default();

        // Should return None when no timing data is available
        assert!(report.main_pass_duration_us().is_none());
        assert!(report.frame_total_duration_us().is_none());
        assert!(report.get_hook_timing_us(GpuPerfHook::FrameStart).is_none());
    }
}
