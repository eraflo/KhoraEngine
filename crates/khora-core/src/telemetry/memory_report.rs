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

//! A report of system memory usage and allocation patterns.

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
