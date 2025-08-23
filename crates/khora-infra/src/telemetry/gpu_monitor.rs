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

//! GPU performance monitoring.

use std::borrow::Cow;
use std::sync::Mutex;

use khora_core::renderer::api::RenderStats;
use khora_core::renderer::GpuHook;
use khora_core::telemetry::monitoring::{
    GpuReport, MonitoredResourceType, ResourceMonitor, ResourceUsageReport,
};

/// GPU performance monitor that works with any RenderSystem implementation.
#[derive(Debug)]
pub struct GpuMonitor {
    system_name: String,
    last_frame_stats: Mutex<Option<GpuReport>>,
}

impl GpuMonitor {
    /// Create a new GPU performance monitor
    pub fn new(system_name: String) -> Self {
        Self {
            system_name,
            last_frame_stats: Mutex::new(None),
        }
    }

    /// Returns the latest detailed GPU performance report.
    pub fn get_gpu_report(&self) -> Option<GpuReport> {
        *self.last_frame_stats.lock().unwrap()
    }

    /// Update performance stats from frame timing data
    pub fn update_from_frame_stats(&self, render_stats: &RenderStats) {
        // Create hook timings based on render stats
        // We simulate the timeline: FrameStart -> MainPassBegin -> MainPassEnd -> FrameEnd
        let frame_start_us = 0u32; // Start of frame timeline
        let frame_end_us = (render_stats.gpu_frame_total_time_ms * 1000.0) as u32;
        let main_pass_duration_us = (render_stats.gpu_main_pass_time_ms * 1000.0) as u32;

        // Place main pass in the middle of the frame for simplicity
        let main_pass_begin_us = (frame_end_us - main_pass_duration_us) / 2;
        let main_pass_end_us = main_pass_begin_us + main_pass_duration_us;

        let mut hook_timings = [None; 4];
        hook_timings[GpuHook::FrameStart as usize] = Some(frame_start_us);
        hook_timings[GpuHook::MainPassBegin as usize] = Some(main_pass_begin_us);
        hook_timings[GpuHook::MainPassEnd as usize] = Some(main_pass_end_us);
        hook_timings[GpuHook::FrameEnd as usize] = Some(frame_end_us);

        let report = GpuReport {
            frame_number: render_stats.frame_number,
            hook_timings_us: hook_timings,
            // Convert milliseconds to microseconds
            cpu_preparation_time_us: Some((render_stats.cpu_preparation_time_ms * 1000.0) as u32),
            cpu_submission_time_us: Some(
                (render_stats.cpu_render_submission_time_ms * 1000.0) as u32,
            ),
        };

        let mut last_stats = self.last_frame_stats.lock().unwrap();
        *last_stats = Some(report);
    }
}

impl ResourceMonitor for GpuMonitor {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Owned(format!("Gpu_{}", self.system_name))
    }

    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::Gpu
    }

    fn get_usage_report(&self) -> ResourceUsageReport {
        // GPU performance doesn't have byte-based usage, so return default
        ResourceUsageReport::default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn update(&self) {
        // GPU monitor updates are handled by update_from_frame_stats()
        // when called from the render system, so no additional work needed here
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_monitor_creation() {
        let monitor = GpuMonitor::new("TestGPU".to_string());
        assert_eq!(monitor.monitor_id(), "Gpu_TestGPU");
        assert_eq!(monitor.resource_type(), MonitoredResourceType::Gpu);
    }

    #[test]
    fn gpu_monitor_update_stats() {
        let monitor = GpuMonitor::new("TestGPU".to_string());

        // Initially no performance report
        assert!(monitor.get_gpu_report().is_none());

        // Create sample render stats
        let render_stats = RenderStats {
            frame_number: 42,
            cpu_preparation_time_ms: 1.0,
            cpu_render_submission_time_ms: 0.5,
            gpu_main_pass_time_ms: 16.67,
            gpu_frame_total_time_ms: 16.67,
            draw_calls: 100,
            triangles_rendered: 1000,
            vram_usage_estimate_mb: 256.0,
        };

        // Update stats
        monitor.update_from_frame_stats(&render_stats);

        // Should now have a performance report
        let report = monitor.get_gpu_report();
        assert!(report.is_some());

        let report = report.unwrap();
        assert_eq!(report.frame_number, 42);
        assert_eq!(report.cpu_preparation_time_us, Some(1000)); // 1ms = 1000μs
        assert_eq!(report.cpu_submission_time_us, Some(500)); // 0.5ms = 500μs
    }

    #[test]
    fn gpu_report_hook_methods() {
        let monitor = GpuMonitor::new("TestGPU".to_string());

        let render_stats = RenderStats {
            frame_number: 1,
            cpu_preparation_time_ms: 0.1,
            cpu_render_submission_time_ms: 0.05,
            gpu_main_pass_time_ms: 16.67,
            gpu_frame_total_time_ms: 17.0,
            draw_calls: 50,
            triangles_rendered: 500,
            vram_usage_estimate_mb: 128.0,
        };

        monitor.update_from_frame_stats(&render_stats);
        let report = monitor.get_gpu_report().unwrap();

        // Test frame number
        assert_eq!(report.frame_number, 1);

        // With our RenderStats-based hook calculation, we now have hook timings
        assert_eq!(report.frame_total_duration_us(), Some(17000)); // 17ms = 17000μs
        assert_eq!(report.main_pass_duration_us(), Some(16670)); // 16.67ms = 16670μs
    }

    #[test]
    fn gpu_report_missing_data() {
        let monitor = GpuMonitor::new("TestGPU".to_string());

        let render_stats = RenderStats {
            frame_number: 1,
            cpu_preparation_time_ms: 0.0,
            cpu_render_submission_time_ms: 0.0,
            gpu_main_pass_time_ms: 0.0,
            gpu_frame_total_time_ms: 0.0,
            draw_calls: 0,
            triangles_rendered: 0,
            vram_usage_estimate_mb: 0.0,
        };

        monitor.update_from_frame_stats(&render_stats);
        let report = monitor.get_gpu_report().unwrap();

        // With zero timing values, hook timings will still be calculated (starting at 0)
        assert_eq!(report.frame_total_duration_us(), Some(0)); // 0ms = 0μs
        assert_eq!(report.main_pass_duration_us(), Some(0)); // 0ms = 0μs
    }
}
