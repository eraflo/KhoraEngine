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

//! Public interface for querying engine-wide memory statistics.
//!
//! This module defines the global atomic counters for memory tracking and provides
//! a public API to read them in a structured way. The counters themselves are
//_ updated by a concrete allocator implementation (like `SaaTrackingAllocator` in `khora-data`).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// --- Global Memory Counters ---
// These statics are the public "contract". Any part of the engine can read them.
// Only the allocator implementation is allowed to write to them.

/// Tracks the total number of bytes currently allocated by the registered global allocator.
pub static CURRENTLY_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Tracks the peak number of bytes ever allocated simultaneously.
pub static PEAK_ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of allocation calls.
pub static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of deallocation calls.
pub static TOTAL_DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of reallocation calls.
pub static TOTAL_REALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes ever allocated.
pub static BYTES_ALLOCATED_LIFETIME: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes ever deallocated.
pub static BYTES_DEALLOCATED_LIFETIME: AtomicU64 = AtomicU64::new(0);

/// Tracks the number of "large" allocations (e.g., >= 1MB).
pub static LARGE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes from "large" allocations.
pub static LARGE_ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

/// Tracks the number of "small" allocations (e.g., < 1KB).
pub static SMALL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes from "small" allocations.
pub static SMALL_ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

// --- Data Structures for Reporting ---

/// A snapshot of comprehensive memory allocation statistics.
#[derive(Debug, Clone, Copy, Default)]
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
    /// Calculate derived metrics from raw counters.
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

// --- Public API for Reading Stats ---

/// Gets comprehensive memory allocation statistics by reading the global counters.
pub fn get_extended_memory_stats() -> ExtendedMemoryStats {
    let current_allocated = CURRENTLY_ALLOCATED_BYTES.load(Ordering::Relaxed);
    let peak_allocated = PEAK_ALLOCATED_BYTES.load(Ordering::Relaxed);
    let total_allocs = TOTAL_ALLOCATIONS.load(Ordering::Relaxed);
    let total_deallocs = TOTAL_DEALLOCATIONS.load(Ordering::Relaxed);
    let total_reallocs = TOTAL_REALLOCATIONS.load(Ordering::Relaxed);
    let bytes_alloc_lifetime = BYTES_ALLOCATED_LIFETIME.load(Ordering::Relaxed);
    let bytes_dealloc_lifetime = BYTES_DEALLOCATED_LIFETIME.load(Ordering::Relaxed);
    let large_allocs = LARGE_ALLOCATIONS.load(Ordering::Relaxed);
    let large_alloc_bytes = LARGE_ALLOCATION_BYTES.load(Ordering::Relaxed);
    let small_allocs = SMALL_ALLOCATIONS.load(Ordering::Relaxed);
    let small_alloc_bytes = SMALL_ALLOCATION_BYTES.load(Ordering::Relaxed);

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
        ..Default::default()
    };

    stats.calculate_derived_metrics();
    stats
}

/// Gets the total number of bytes currently allocated via the global allocator.
pub fn get_currently_allocated_bytes() -> usize {
    CURRENTLY_ALLOCATED_BYTES.load(Ordering::Relaxed)
}
