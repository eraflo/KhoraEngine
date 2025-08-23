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

//! Provides traits and data structures for active resource monitoring.
//!
//! "Monitoring" is distinct from "metrics" in that it involves actively polling
//! a system resource (like VRAM or a GPU) to get a snapshot of its state, whereas
//! metrics are typically discrete, event-based measurements.

use std::borrow::Cow;
use std::fmt::Debug;

use crate::renderer::GpuHook;

/// The core trait for a resource monitor.
///
/// A `ResourceMonitor` is a stateful object, typically living in the `khora-infra`
/// crate, that knows how to query a specific system resource. The `khora-telemetry`
/// service will hold a collection of these monitors and periodically call `update`
/// and `get_usage_report` on them.
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    /// Returns a unique, human-readable identifier for this monitor instance.
    fn monitor_id(&self) -> Cow<'static, str>;

    /// Returns the general type of resource being monitored.
    fn resource_type(&self) -> MonitoredResourceType;

    /// Returns a snapshot of the current usage data for the monitored resource.
    fn get_usage_report(&self) -> ResourceUsageReport;

    /// Allows downcasting to a concrete `ResourceMonitor` type.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Triggers the monitor to update its internal state by polling the resource.
    /// This default implementation does nothing, for monitors that update passively.
    fn update(&self) {
        // Default: no-op
    }
}

/// An enumeration of the types of resources that can be monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoredResourceType {
    /// Video RAM on a GPU.
    Vram,
    /// Main system RAM.
    SystemRam,
    /// General GPU performance (e.g., execution timing).
    Gpu,
}

/// A generic, unified report of resource usage, typically in bytes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsageReport {
    /// The number of bytes currently in use.
    pub current_bytes: u64,
    /// The peak number of bytes ever in use simultaneously, if tracked.
    pub peak_bytes: Option<u64>,
    /// The total capacity of the resource in bytes, if known.
    pub total_capacity_bytes: Option<u64>,
}

/// A report of GPU performance timings for a single frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuReport {
    /// The frame number this report corresponds to.
    pub frame_number: u64,
    /// Raw timestamp query results for each GPU hook, in microseconds.
    /// The order corresponds to the `GpuHook` enum definition.
    pub hook_timings_us: [Option<u32>; 4],
    /// The CPU time spent preparing the frame, in microseconds.
    pub cpu_preparation_time_us: Option<u32>,
    /// The CPU time spent submitting commands for the frame, in microseconds.
    pub cpu_submission_time_us: Option<u32>,
}

/// A detailed report of system memory (RAM) usage and allocation patterns.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryReport {
    /// The number of bytes of system RAM currently in use by the application.
    pub current_usage_bytes: usize,
    /// The peak number of bytes of system RAM ever used simultaneously.
    pub peak_usage_bytes: usize,
    /// The number of bytes allocated since the last monitor update.
    pub allocation_delta_bytes: usize,
    /// The total number of times the memory usage has been sampled.
    pub sample_count: u64,

    // Extended statistics (often from a tracking allocator)
    /// The total number of allocation calls since the start.
    pub total_allocations: u64,
    /// The total number of deallocation calls since the start.
    pub total_deallocations: u64,
    /// The total number of reallocation calls since the start.
    pub total_reallocations: u64,
    /// The cumulative sum of all bytes ever allocated.
    pub bytes_allocated_lifetime: u64,
    /// The cumulative sum of all bytes ever deallocated.
    pub bytes_deallocated_lifetime: u64,
    /// The number of allocations classified as "large" (e.g., >= 1MB).
    pub large_allocations: u64,
    /// The total byte size of all "large" allocations.
    pub large_allocation_bytes: u64,
    /// The number of allocations classified as "small" (e.g., < 1KB).
    pub small_allocations: u64,
    /// The total byte size of all "small" allocations.
    pub small_allocation_bytes: u64,
    /// A calculated ratio indicating potential memory fragmentation.
    pub fragmentation_ratio: f64,
    /// A calculated ratio of memory still in use versus total ever allocated.
    pub allocation_efficiency: f64,
    /// The calculated average size of a single memory allocation in bytes.
    pub average_allocation_size: f64,
}

/// A report of Video RAM (VRAM) usage.
#[derive(Debug, Clone, Copy, Default)]
pub struct VramReport {
    /// The number of bytes of VRAM currently in use.
    pub current_usage_bytes: usize,
    /// The peak number of bytes of VRAM ever in use, if tracked.
    pub peak_usage_bytes: Option<usize>,
    /// The total physical VRAM capacity in bytes, if available.
    pub total_capacity_bytes: Option<usize>,
}

/// A trait for types that can provide VRAM usage statistics.
/// This is typically implemented by a `GraphicsDevice` or a dedicated monitor in `khora-infra`.
pub trait VramProvider: Send + Sync {
    /// Returns the current VRAM usage in megabytes.
    fn get_vram_usage_mb(&self) -> f32;
    /// Returns the peak VRAM usage in megabytes.
    fn get_vram_peak_mb(&self) -> f32;
    /// Returns the total VRAM capacity in megabytes, if available.
    fn get_vram_capacity_mb(&self) -> Option<f32>;
}

impl MemoryReport {
    /// Returns the current memory usage in megabytes (MB).
    pub fn current_usage_mb(&self) -> f64 {
        self.current_usage_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Returns the peak memory usage in megabytes (MB).
    pub fn peak_usage_mb(&self) -> f64 {
        self.peak_usage_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Returns the change in allocated bytes since the last update, in kilobytes (KB).
    pub fn allocation_delta_kb(&self) -> f64 {
        self.allocation_delta_bytes as f64 / 1024.0
    }

    /// Calculates the memory turnover rate (allocations + deallocations per sample).
    pub fn memory_turnover_rate(&self) -> f64 {
        if self.sample_count > 0 {
            (self.total_allocations + self.total_deallocations) as f64 / self.sample_count as f64
        } else {
            0.0
        }
    }

    /// Calculates the percentage of total allocations that were classified as "large".
    pub fn large_allocation_percentage(&self) -> f64 {
        if self.total_allocations > 0 {
            (self.large_allocations as f64 / self.total_allocations as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Returns the memory allocation efficiency as a percentage.
    pub fn memory_utilization_efficiency(&self) -> f64 {
        self.allocation_efficiency * 100.0
    }

    /// Returns the average allocation size in megabytes (MB).
    pub fn average_allocation_size_mb(&self) -> f64 {
        self.average_allocation_size / (1024.0 * 1024.0)
    }

    /// Returns a descriptive string for the current fragmentation status.
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
    /// Gets the timing for a specific GPU performance hook, in microseconds.
    pub fn get_hook_timing_us(&self, hook: GpuHook) -> Option<u32> {
        self.hook_timings_us[hook as usize]
    }

    /// Calculates the duration of the main render pass, in microseconds.
    pub fn main_pass_duration_us(&self) -> Option<u32> {
        match (
            self.get_hook_timing_us(GpuHook::MainPassBegin),
            self.get_hook_timing_us(GpuHook::MainPassEnd),
        ) {
            (Some(begin), Some(end)) if end >= begin => Some(end - begin),
            _ => None,
        }
    }

    /// Calculates the total GPU duration for the frame, in microseconds.
    pub fn frame_total_duration_us(&self) -> Option<u32> {
        match (
            self.get_hook_timing_us(GpuHook::FrameStart),
            self.get_hook_timing_us(GpuHook::FrameEnd),
        ) {
            (Some(start), Some(end)) if end >= start => Some(end - start),
            _ => None,
        }
    }

    /// Sets the timing for a specific hook, in microseconds.
    pub fn set_hook_timing_us(&mut self, hook: GpuHook, timing_us: Option<u32>) {
        self.hook_timings_us[hook as usize] = timing_us;
    }
}
