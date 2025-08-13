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
use std::fmt::Debug;

/// Corresponds to the type of resource being monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoredResourceType {
    Vram,
    SystemRam,
    GpuPerformance,
}

/// Represents the current usage and limit of a monitored resource.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsageReport {
    pub current_bytes: u64,
    pub peak_bytes: Option<u64>,
    pub total_capacity_bytes: Option<u64>,
}

/// Represents GPU performance metrics (times in microseconds for higher precision).
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuPerformanceReport {
    /// Frame number for tracking
    pub frame_number: u64,
    /// Individual hook timings in microseconds (indexed by GpuPerfHook as usize)
    /// [FrameStart, MainPassBegin, MainPassEnd, FrameEnd]
    pub hook_timings_us: [Option<u32>; 4],
    /// CPU preparation time in microseconds
    pub cpu_preparation_time_us: Option<u32>,
    /// CPU submission time in microseconds
    pub cpu_submission_time_us: Option<u32>,
}

impl GpuPerformanceReport {
    /// Get timing for a specific GPU performance hook
    pub fn get_hook_timing_us(
        &self,
        hook: crate::subsystems::renderer::api::common_types::GpuPerfHook,
    ) -> Option<u32> {
        self.hook_timings_us[hook as usize]
    }

    /// Get main pass duration (MainPassEnd - MainPassBegin)
    pub fn main_pass_duration_us(&self) -> Option<u32> {
        use crate::subsystems::renderer::api::common_types::GpuPerfHook;
        match (
            self.get_hook_timing_us(GpuPerfHook::MainPassBegin),
            self.get_hook_timing_us(GpuPerfHook::MainPassEnd),
        ) {
            (Some(begin), Some(end)) if end >= begin => Some(end - begin),
            _ => None,
        }
    }

    /// Get total frame duration (FrameEnd - FrameStart)  
    pub fn frame_total_duration_us(&self) -> Option<u32> {
        use crate::subsystems::renderer::api::common_types::GpuPerfHook;
        match (
            self.get_hook_timing_us(GpuPerfHook::FrameStart),
            self.get_hook_timing_us(GpuPerfHook::FrameEnd),
        ) {
            (Some(start), Some(end)) if end >= start => Some(end - start),
            _ => None,
        }
    }

    /// Set timing for a specific hook
    pub fn set_hook_timing_us(
        &mut self,
        hook: crate::subsystems::renderer::api::common_types::GpuPerfHook,
        timing_us: Option<u32>,
    ) {
        self.hook_timings_us[hook as usize] = timing_us;
    }
}

/// The `ResourceMonitor` trait provides methods to monitor resources in a system.
/// It includes methods to get the ID of the monitor, the type of resource being monitored,
/// and a report of the current usage and limit of the resource.
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    fn monitor_id(&self) -> Cow<'static, str>;
    fn resource_type(&self) -> MonitoredResourceType;
    fn get_usage_report(&self) -> ResourceUsageReport;

    /// Optional: Get GPU performance report for performance monitors
    fn get_gpu_performance_report(&self) -> Option<GpuPerformanceReport> {
        None
    }
}
