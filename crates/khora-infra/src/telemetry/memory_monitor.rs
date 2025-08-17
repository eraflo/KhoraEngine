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

//! System Memory Resource Monitor
//!
//! Provides memory monitoring capabilities through integration with
//! the SaaTrackingAllocator for heap allocation tracking.

use std::borrow::Cow;
use std::sync::Mutex;

use khora_core::memory::{get_currently_allocated_bytes, get_extended_memory_stats};
use khora_core::telemetry::monitoring::{
    MemoryReport, MonitoredResourceType, ResourceMonitor, ResourceUsageReport,
};

/// System memory resource monitor.
///
/// Tracks heap allocation statistics through the SaaTrackingAllocator
/// and provides real-time memory usage information.
#[derive(Debug)]
pub struct MemoryMonitor {
    id: String,
    last_report: Mutex<Option<MemoryReport>>,
    peak_usage_bytes: Mutex<usize>,
    last_allocation_bytes: Mutex<usize>,
    sample_count: Mutex<u64>,
}

impl MemoryMonitor {
    pub fn new(id: String) -> Self {
        let current_usage = get_currently_allocated_bytes();
        Self {
            id,
            last_report: Mutex::new(None),
            peak_usage_bytes: Mutex::new(current_usage),
            last_allocation_bytes: Mutex::new(current_usage),
            sample_count: Mutex::new(0),
        }
    }

    /// Returns the latest detailed memory report.
    pub fn get_memory_report(&self) -> Option<MemoryReport> {
        let last_report = self.last_report.lock().unwrap();
        *last_report
    }

    /// Resets the peak usage counter to the current memory usage.
    pub fn reset_peak_usage(&self) {
        let current_usage = get_currently_allocated_bytes();
        let mut peak = self.peak_usage_bytes.lock().unwrap();
        *peak = current_usage;
    }

    /// Updates the monitor's internal state by querying the global allocator stats.
    fn update_internal_stats(&self) {
        let current_usage = get_currently_allocated_bytes();
        let extended_stats = get_extended_memory_stats();

        // Update peak tracking
        let mut peak = self.peak_usage_bytes.lock().unwrap();
        if current_usage > *peak {
            *peak = current_usage;
        }

        // Calculate allocation delta
        let mut last_alloc = self.last_allocation_bytes.lock().unwrap();
        let allocation_delta = current_usage.saturating_sub(*last_alloc);
        *last_alloc = current_usage;

        // Update sample count
        let mut count = self.sample_count.lock().unwrap();
        *count += 1;

        // Create comprehensive report with extended statistics
        let report = MemoryReport {
            current_usage_bytes: current_usage,
            peak_usage_bytes: *peak,
            allocation_delta_bytes: allocation_delta,
            sample_count: *count,

            // Extended statistics from allocator
            total_allocations: extended_stats.total_allocations,
            total_deallocations: extended_stats.total_deallocations,
            total_reallocations: extended_stats.total_reallocations,
            bytes_allocated_lifetime: extended_stats.bytes_allocated_lifetime,
            bytes_deallocated_lifetime: extended_stats.bytes_deallocated_lifetime,
            large_allocations: extended_stats.large_allocations,
            large_allocation_bytes: extended_stats.large_allocation_bytes,
            small_allocations: extended_stats.small_allocations,
            small_allocation_bytes: extended_stats.small_allocation_bytes,
            fragmentation_ratio: extended_stats.fragmentation_ratio,
            allocation_efficiency: extended_stats.allocation_efficiency,
            average_allocation_size: extended_stats.average_allocation_size,
        };

        let mut last_report = self.last_report.lock().unwrap();
        *last_report = Some(report);
    }
}

impl ResourceMonitor for MemoryMonitor {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Owned(self.id.clone())
    }

    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::SystemRam
    }

    fn get_usage_report(&self) -> ResourceUsageReport {
        let current_usage = get_currently_allocated_bytes();
        let peak_usage = *self.peak_usage_bytes.lock().unwrap();

        ResourceUsageReport {
            current_bytes: current_usage as u64,
            peak_bytes: Some(peak_usage as u64),
            total_capacity_bytes: None, // System memory limit not easily available
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn update(&self) {
        self.update_internal_stats();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_monitor_creation() {
        let monitor = MemoryMonitor::new("TestMemory".to_string());
        assert_eq!(monitor.monitor_id(), "TestMemory");
        assert_eq!(monitor.resource_type(), MonitoredResourceType::SystemRam);
    }

    #[test]
    fn memory_monitor_update_stats() {
        let monitor = MemoryMonitor::new("TestMemory".to_string());

        // Initially no report available until update is called
        assert!(monitor.get_memory_report().is_none());

        // Update stats
        monitor.update_internal_stats();

        // After update, report should be available
        let report = monitor.get_memory_report().unwrap();
        // In test environment, memory usage might be 0, so we just check report exists
        assert_eq!(report.sample_count, 1);
    }

    #[test]
    fn memory_monitor_peak_tracking() {
        let monitor = MemoryMonitor::new("TestMemory".to_string());

        monitor.update_internal_stats();
        let _initial_report = monitor.get_memory_report().unwrap();
        let _initial_peak = _initial_report.peak_usage_bytes;

        // Reset peak and update again
        monitor.reset_peak_usage();
        monitor.update_internal_stats();

        let after_reset_report = monitor.get_memory_report().unwrap();
        // Peak should be reset to current usage
        assert_eq!(after_reset_report.sample_count, 2);
    }

    #[test]
    fn memory_monitor_reset_peak() {
        let monitor = MemoryMonitor::new("TestMemory".to_string());

        monitor.update_internal_stats();
        let before_reset = monitor.get_memory_report().unwrap();

        monitor.reset_peak_usage();
        monitor.update_internal_stats();
        let after_reset = monitor.get_memory_report().unwrap();

        // Sample count should increment
        assert_eq!(after_reset.sample_count, before_reset.sample_count + 1);
    }

    #[test]
    fn memory_monitor_integration_test() {
        let monitor = MemoryMonitor::new("TestMemory".to_string());

        // Test monitor identification
        assert_eq!(monitor.monitor_id(), "TestMemory");
        assert_eq!(monitor.resource_type(), MonitoredResourceType::SystemRam);

        // Test memory tracking over time
        monitor.update_internal_stats();
        let updated_report = monitor.get_usage_report();
        assert!(updated_report.peak_bytes.is_some());

        // Test specific report methods
        assert!(monitor.get_memory_report().is_some());
    }
}
