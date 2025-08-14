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

use std::sync::atomic::Ordering;

pub mod allocator;

pub use allocator::SaaTrackingAllocator;

/// Comprehensive memory allocation statistics
#[derive(Debug, Clone, Copy)]
pub struct ExtendedMemoryStats {
    // Current state
    pub current_allocated_bytes: usize,
    pub peak_allocated_bytes: u64,

    // Allocation counters
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub total_reallocations: u64,
    pub net_allocations: i64,

    // Lifetime totals
    pub bytes_allocated_lifetime: u64,
    pub bytes_deallocated_lifetime: u64,
    pub bytes_net_lifetime: i64,

    // Size category tracking
    pub large_allocations: u64,
    pub large_allocation_bytes: u64,
    pub small_allocations: u64,
    pub small_allocation_bytes: u64,
    pub medium_allocations: u64,
    pub medium_allocation_bytes: u64,

    // Calculated metrics
    pub average_allocation_size: f64,
    pub fragmentation_ratio: f64,
    pub allocation_efficiency: f64,
}

impl ExtendedMemoryStats {
    /// Calculate derived metrics from raw counters
    pub fn calculate_derived_metrics(&mut self) {
        // Average allocation size
        if self.total_allocations > 0 {
            self.average_allocation_size =
                self.bytes_allocated_lifetime as f64 / self.total_allocations as f64;
        }

        // Fragmentation ratio (peak vs current)
        if self.peak_allocated_bytes > 0 {
            self.fragmentation_ratio =
                1.0 - (self.current_allocated_bytes as f64 / self.peak_allocated_bytes as f64);
        }

        // Allocation efficiency (how much of allocated memory is still in use)
        if self.bytes_allocated_lifetime > 0 {
            self.allocation_efficiency =
                self.current_allocated_bytes as f64 / self.bytes_allocated_lifetime as f64;
        }

        // Calculate medium allocations (everything that's not small or large)
        self.medium_allocations =
            self.total_allocations - self.small_allocations - self.large_allocations;
        self.medium_allocation_bytes = self.bytes_allocated_lifetime
            - self.small_allocation_bytes
            - self.large_allocation_bytes;
    }
}

/// Gets comprehensive memory allocation statistics
pub fn get_extended_memory_stats() -> ExtendedMemoryStats {
    let current_allocated = allocator::ALLOCATED_BYTES.load(Ordering::Relaxed);
    let peak_allocated = allocator::PEAK_ALLOCATED_BYTES.load(Ordering::Relaxed);
    let total_allocs = allocator::TOTAL_ALLOCATIONS.load(Ordering::Relaxed);
    let total_deallocs = allocator::TOTAL_DEALLOCATIONS.load(Ordering::Relaxed);
    let total_reallocs = allocator::TOTAL_REALLOCATIONS.load(Ordering::Relaxed);
    let bytes_alloc_lifetime = allocator::BYTES_ALLOCATED_LIFETIME.load(Ordering::Relaxed);
    let bytes_dealloc_lifetime = allocator::BYTES_DEALLOCATED_LIFETIME.load(Ordering::Relaxed);
    let large_allocs = allocator::LARGE_ALLOCATIONS.load(Ordering::Relaxed);
    let large_alloc_bytes = allocator::LARGE_ALLOCATION_BYTES.load(Ordering::Relaxed);
    let small_allocs = allocator::SMALL_ALLOCATIONS.load(Ordering::Relaxed);
    let small_alloc_bytes = allocator::SMALL_ALLOCATION_BYTES.load(Ordering::Relaxed);

    let mut stats = ExtendedMemoryStats {
        current_allocated_bytes: current_allocated,
        peak_allocated_bytes: peak_allocated,
        total_allocations: total_allocs,
        total_deallocations: total_deallocs,
        total_reallocations: total_reallocs,
        net_allocations: total_allocs as i64 - total_deallocs as i64,
        bytes_allocated_lifetime: bytes_alloc_lifetime,
        bytes_deallocated_lifetime: bytes_dealloc_lifetime,
        bytes_net_lifetime: bytes_alloc_lifetime as i64 - bytes_dealloc_lifetime as i64,
        large_allocations: large_allocs,
        large_allocation_bytes: large_alloc_bytes,
        small_allocations: small_allocs,
        small_allocation_bytes: small_alloc_bytes,
        medium_allocations: 0,        // Will be calculated
        medium_allocation_bytes: 0,   // Will be calculated
        average_allocation_size: 0.0, // Will be calculated
        fragmentation_ratio: 0.0,     // Will be calculated
        allocation_efficiency: 0.0,   // Will be calculated
    };

    stats.calculate_derived_metrics();
    stats
}

/// Gets the total number of bytes currently allocated via the global allocator.
/// This function provides a way to monitor memory usage in the application.
/// ## Returns
/// The total number of bytes currently allocated.
pub fn get_currently_allocated_bytes() -> usize {
    allocator::ALLOCATED_BYTES.load(Ordering::Relaxed)
}
