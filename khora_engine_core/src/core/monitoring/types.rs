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

//! Monitoring data types and enums.

/// Type of resource being monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoredResourceType {
    Vram,
    SystemRam,
    Gpu,
}

/// General resource usage report used by all monitors.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsageReport {
    pub current_bytes: u64,
    pub peak_bytes: Option<u64>,
    pub total_capacity_bytes: Option<u64>,
}

/// GPU performance metrics (times in microseconds).
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuReport {
    /// Frame number for tracking
    pub frame_number: u64,
    /// Individual hook timings in microseconds (indexed by GpuHook as usize)
    /// [FrameStart, MainPassBegin, MainPassEnd, FrameEnd]
    pub hook_timings_us: [Option<u32>; 4],
    /// CPU preparation time in microseconds
    pub cpu_preparation_time_us: Option<u32>,
    /// CPU submission time in microseconds
    pub cpu_submission_time_us: Option<u32>,
}

/// System memory usage metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryReport {
    /// Current memory usage in bytes
    pub current_usage_bytes: usize,
    /// Peak memory usage since monitor creation or reset
    pub peak_usage_bytes: usize,
    /// Bytes allocated since last update (allocation rate indicator)
    pub allocation_delta_bytes: usize,
    /// Number of samples taken since monitor creation
    pub sample_count: u64,

    // Extended statistics
    /// Total number of allocations since start
    pub total_allocations: u64,
    /// Total number of deallocations since start
    pub total_deallocations: u64,
    /// Total number of reallocations since start
    pub total_reallocations: u64,
    /// Total bytes allocated during lifetime
    pub bytes_allocated_lifetime: u64,
    /// Total bytes deallocated during lifetime
    pub bytes_deallocated_lifetime: u64,
    /// Number of large allocations (>= 1MB)
    pub large_allocations: u64,
    /// Bytes from large allocations
    pub large_allocation_bytes: u64,
    /// Number of small allocations (< 1MB)
    pub small_allocations: u64,
    /// Bytes from small allocations
    pub small_allocation_bytes: u64,
    /// Memory fragmentation ratio (0.0 = no fragmentation, 1.0 = high fragmentation)
    pub fragmentation_ratio: f64,
    /// Allocation efficiency (allocated bytes / requested bytes)
    pub allocation_efficiency: f64,
    /// Average allocation size in bytes
    pub average_allocation_size: f64,
}

/// VRAM usage metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct VramReport {
    /// Current VRAM usage in bytes
    pub current_usage_bytes: usize,
    /// Peak VRAM usage in bytes (if tracked)
    pub peak_usage_bytes: Option<usize>,
    /// Total VRAM capacity in bytes (if available)
    pub total_capacity_bytes: Option<usize>,
}

impl MemoryReport {
    /// Get current usage in megabytes
    pub fn current_usage_mb(&self) -> f64 {
        self.current_usage_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get peak usage in megabytes
    pub fn peak_usage_mb(&self) -> f64 {
        self.peak_usage_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get allocation delta in kilobytes
    pub fn allocation_delta_kb(&self) -> f64 {
        self.allocation_delta_bytes as f64 / 1024.0
    }

    /// Get memory turnover rate (allocations + deallocations per sample)
    pub fn memory_turnover_rate(&self) -> f64 {
        if self.sample_count > 0 {
            (self.total_allocations + self.total_deallocations) as f64 / self.sample_count as f64
        } else {
            0.0
        }
    }

    /// Get large allocation percentage
    pub fn large_allocation_percentage(&self) -> f64 {
        if self.total_allocations > 0 {
            (self.large_allocations as f64 / self.total_allocations as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get memory utilization efficiency percentage
    pub fn memory_utilization_efficiency(&self) -> f64 {
        self.allocation_efficiency * 100.0
    }

    /// Get average allocation size in megabytes
    pub fn average_allocation_size_mb(&self) -> f64 {
        self.average_allocation_size / (1024.0 * 1024.0)
    }

    /// Get fragmentation status description
    pub fn fragmentation_status(&self) -> &'static str {
        match self.fragmentation_ratio {
            r if r < 0.1 => "Low",
            r if r < 0.3 => "Moderate",
            r if r < 0.6 => "High",
            _ => "Critical",
        }
    }
}

impl GpuReport {
    /// Get timing for a specific GPU performance hook
    pub fn get_hook_timing_us(
        &self,
        hook: crate::subsystems::renderer::api::common_types::GpuHook,
    ) -> Option<u32> {
        self.hook_timings_us[hook as usize]
    }

    /// Get main pass duration (MainPassEnd - MainPassBegin)
    pub fn main_pass_duration_us(&self) -> Option<u32> {
        use crate::subsystems::renderer::api::common_types::GpuHook;
        match (
            self.get_hook_timing_us(GpuHook::MainPassBegin),
            self.get_hook_timing_us(GpuHook::MainPassEnd),
        ) {
            (Some(begin), Some(end)) if end >= begin => Some(end - begin),
            _ => None,
        }
    }

    /// Get total frame duration (FrameEnd - FrameStart)  
    pub fn frame_total_duration_us(&self) -> Option<u32> {
        use crate::subsystems::renderer::api::common_types::GpuHook;
        match (
            self.get_hook_timing_us(GpuHook::FrameStart),
            self.get_hook_timing_us(GpuHook::FrameEnd),
        ) {
            (Some(start), Some(end)) if end >= start => Some(end - start),
            _ => None,
        }
    }

    /// Set timing for a specific hook
    pub fn set_hook_timing_us(
        &mut self,
        hook: crate::subsystems::renderer::api::common_types::GpuHook,
        timing_us: Option<u32>,
    ) {
        self.hook_timings_us[hook as usize] = timing_us;
    }
}
