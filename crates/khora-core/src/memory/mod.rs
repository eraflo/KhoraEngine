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

//! Provides a public interface for querying engine-wide memory allocation statistics.
//!
//! This module defines a set of global atomic counters for detailed memory tracking.
//! It forms a "contract" where a registered global allocator is responsible for
//! incrementing these counters, and any part of the engine can read them in a
//! thread-safe manner to monitor memory usage.
//!
//! The primary use case is for the `khora-telemetry` crate to collect these stats
//! and feed them into the Dynamic Context Core (DCC) for adaptive decision-making.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// --- Global Memory Counters ---

/// Tracks the total number of bytes currently allocated by the registered global allocator.
pub static CURRENTLY_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Tracks the peak number of bytes ever allocated simultaneously during the application's lifetime.
pub static PEAK_ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of allocation calls made.
pub static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of deallocation calls made.
pub static TOTAL_DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the total number of reallocation calls made.
pub static TOTAL_REALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes ever allocated over the application's lifetime.
pub static BYTES_ALLOCATED_LIFETIME: AtomicU64 = AtomicU64::new(0);

/// Tracks the cumulative total of bytes ever deallocated over the application's lifetime.
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

/// A snapshot of comprehensive memory allocation statistics, including derived metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtendedMemoryStats {
    // --- Current State ---
    /// The total number of bytes currently in use.
    pub current_allocated_bytes: usize,
    /// The maximum number of bytes that were ever in use simultaneously.
    pub peak_allocated_bytes: u64,

    // --- Allocation Counters ---
    /// The total number of times an allocation was requested.
    pub total_allocations: u64,
    /// The total number of times a deallocation was requested.
    pub total_deallocations: u64,
    /// The total number of times a reallocation was requested.
    pub total_reallocations: u64,
    /// The net number of active allocations (`total_allocations` - `total_deallocations`).
    pub net_allocations: i64,

    // --- Lifetime Totals ---
    /// The cumulative sum of all bytes ever allocated.
    pub bytes_allocated_lifetime: u64,
    /// The cumulative sum of all bytes ever deallocated.
    pub bytes_deallocated_lifetime: u64,
    /// The net number of bytes allocated over the lifetime. Should be equal to `current_allocated_bytes`.
    pub bytes_net_lifetime: i64,

    // --- Size Category Tracking ---
    /// The number of allocations classified as "large".
    pub large_allocations: u64,
    /// The total byte size of all "large" allocations.
    pub large_allocation_bytes: u64,
    /// The number of allocations classified as "small".
    pub small_allocations: u64,
    /// The total byte size of all "small" allocations.
    pub small_allocation_bytes: u64,
    /// The number of allocations not classified as small or large.
    pub medium_allocations: u64,
    /// The total byte size of all "medium" allocations.
    pub medium_allocation_bytes: u64,

    // --- Calculated Metrics ---
    /// The average size of a single allocation (`bytes_allocated_lifetime` / `total_allocations`).
    pub average_allocation_size: f64,
    /// A rough measure of memory fragmentation (`1.0 - current / peak`).
    pub fragmentation_ratio: f64,
    /// The ratio of memory still in use compared to all memory ever allocated.
    pub allocation_efficiency: f64,
}

impl ExtendedMemoryStats {
    /// Populates the derived metrics based on the raw counter values.
    pub fn calculate_derived_metrics(&mut self) {
        if self.total_allocations > 0 {
            self.average_allocation_size =
                self.bytes_allocated_lifetime as f64 / self.total_allocations as f64;
        }

        if self.peak_allocated_bytes > 0 {
            self.fragmentation_ratio =
                1.0 - (self.current_allocated_bytes as f64 / self.peak_allocated_bytes as f64);
        }

        if self.bytes_allocated_lifetime > 0 {
            self.allocation_efficiency =
                self.current_allocated_bytes as f64 / self.bytes_allocated_lifetime as f64;
        }

        self.medium_allocations =
            self.total_allocations - self.small_allocations - self.large_allocations;
        self.medium_allocation_bytes = self.bytes_allocated_lifetime
            - self.small_allocation_bytes
            - self.large_allocation_bytes;
    }
}

// --- Public API for Reading Stats ---

/// Takes a snapshot of all global memory counters and returns them in a structured format.
///
/// This function is the primary entry point for querying memory statistics. It reads
/// all counters atomically (using `Ordering::Relaxed`) and calculates several
/// derived metrics.
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

/// Gets the total number of bytes currently allocated by the global allocator.
///
/// This is a lightweight alternative to `get_extended_memory_stats` for when only
/// the current usage is needed.
pub fn get_currently_allocated_bytes() -> usize {
    CURRENTLY_ALLOCATED_BYTES.load(Ordering::Relaxed)
}
