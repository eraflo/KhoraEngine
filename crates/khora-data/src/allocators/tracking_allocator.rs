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

//! An implementation of `GlobalAlloc` that tracks memory usage.

use khora_core::memory::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::Ordering;

/// The size, in bytes, above which an allocation is considered "large".
const LARGE_ALLOCATION_THRESHOLD: usize = 1024 * 1024; // 1MB
/// The size, in bytes, below which an allocation is considered "small".
const SMALL_ALLOCATION_THRESHOLD: usize = 1024; // 1KB

/// A wrapper around a `GlobalAlloc` implementation (like `std::alloc::System`)
/// that intercepts allocation calls to update the global memory counters defined
/// in `khora_core::memory`.
///
/// This allocator is the key to enabling the SAA's memory monitoring. By registering
/// it as the `#[global_allocator]`, all heap allocations made by the application
/// will be tracked, providing essential telemetry to the Dynamic Context Core (DCC).
///
/// # Type Parameters
///
/// * `A`: The underlying allocator that will perform the actual memory allocation.
///   Defaults to `System`, the standard Rust allocator.
///
/// # Usage
///
/// ```rust,ignore
/// use khora_data::allocators::SaaTrackingAllocator;
///
/// #[global_allocator]
/// static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct SaaTrackingAllocator<A = System> {
    inner: A,
}

impl<A> SaaTrackingAllocator<A> {
    /// Creates a new tracking allocator that wraps the given inner allocator.
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for SaaTrackingAllocator<A> {
    /// Allocates memory and updates tracking counters.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it is part of the `GlobalAlloc` trait.
    /// The caller must ensure that `layout` has a non-zero size.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.inner.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            let result = CURRENTLY_ALLOCATED_BYTES.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |current| current.checked_add(size),
            );

            if let Ok(current_total) = result {
                let new_total = current_total + size;
                PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);
                TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                BYTES_ALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);

                if size >= LARGE_ALLOCATION_THRESHOLD {
                    LARGE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                    LARGE_ALLOCATION_BYTES.fetch_add(size as u64, Ordering::Relaxed);
                } else if size < SMALL_ALLOCATION_THRESHOLD {
                    SMALL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                    SMALL_ALLOCATION_BYTES.fetch_add(size as u64, Ordering::Relaxed);
                }
            } else {
                log::error!("Memory tracking counter overflowed during alloc! Size: {size}");
            }
        }
        ptr
    }

    /// Deallocates memory and updates tracking counters.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it is part of the `GlobalAlloc` trait.
    /// The caller must ensure that `ptr` was allocated by this allocator with the same `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let result = CURRENTLY_ALLOCATED_BYTES.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| current.checked_sub(size),
        );

        if result.is_err() {
            log::error!("Memory tracking counter underflowed during dealloc! Size: {size}");
        } else {
            TOTAL_DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            BYTES_DEALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);
        }

        self.inner.dealloc(ptr, layout);
    }

    /// Allocates zero-initialized memory and updates tracking counters.
    ///
    /// # Safety
    ///
    /// This function is unsafe for the same reasons as `alloc`.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = self.inner.alloc_zeroed(layout);
        if !ptr.is_null() {
            let size = layout.size();
            let result = CURRENTLY_ALLOCATED_BYTES.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |current| current.checked_add(size),
            );

            if let Ok(current_total) = result {
                let new_total = current_total + size;
                PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);
                TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                BYTES_ALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);

                if size >= LARGE_ALLOCATION_THRESHOLD {
                    LARGE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                    LARGE_ALLOCATION_BYTES.fetch_add(size as u64, Ordering::Relaxed);
                } else if size < SMALL_ALLOCATION_THRESHOLD {
                    SMALL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                    SMALL_ALLOCATION_BYTES.fetch_add(size as u64, Ordering::Relaxed);
                }
            } else {
                log::error!("Memory tracking counter overflowed during alloc_zeroed! Size: {size}");
            }
        }
        ptr
    }

    /// Reallocates memory and updates tracking counters.
    ///
    /// # Safety
    ///
    /// This function is unsafe for the same reasons as `realloc` in `GlobalAlloc`.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_size = layout.size();
        let new_ptr = self.inner.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            TOTAL_REALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            let size_diff = new_size as isize - old_size as isize;
            let fetch_result = match size_diff.cmp(&0) {
                std::cmp::Ordering::Greater => {
                    let additional_bytes = size_diff as usize;
                    BYTES_ALLOCATED_LIFETIME.fetch_add(additional_bytes as u64, Ordering::Relaxed);
                    CURRENTLY_ALLOCATED_BYTES.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| current.checked_add(additional_bytes),
                    )
                }
                std::cmp::Ordering::Less => {
                    let freed_bytes = (-size_diff) as usize;
                    BYTES_DEALLOCATED_LIFETIME.fetch_add(freed_bytes as u64, Ordering::Relaxed);
                    CURRENTLY_ALLOCATED_BYTES.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| current.checked_sub(freed_bytes),
                    )
                }
                std::cmp::Ordering::Equal => Ok(CURRENTLY_ALLOCATED_BYTES.load(Ordering::Relaxed)),
            };

            if size_diff > 0 {
                if let Ok(new_total) = fetch_result {
                    PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);
                }
            }

            if fetch_result.is_err() {
                log::error!(
                    "Memory tracking counter overflow/underflow during realloc! Diff: {size_diff}"
                );
            }
        }
        new_ptr
    }
}
