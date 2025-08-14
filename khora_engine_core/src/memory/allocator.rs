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

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Static atomic counter to hold the currently allocated bytes total.
/// This is kept separate from the allocator struct instance for easier global access.
pub(crate) static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Extended memory tracking statistics
pub(crate) static PEAK_ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
pub(crate) static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static TOTAL_DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static TOTAL_REALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static BYTES_ALLOCATED_LIFETIME: AtomicU64 = AtomicU64::new(0);
pub(crate) static BYTES_DEALLOCATED_LIFETIME: AtomicU64 = AtomicU64::new(0);

/// Large allocation tracking (>= 1MB)
pub(crate) static LARGE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static LARGE_ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

/// Small allocation tracking (< 1KB)
pub(crate) static SMALL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SMALL_ALLOCATION_BYTES: AtomicU64 = AtomicU64::new(0);

const LARGE_ALLOCATION_THRESHOLD: usize = 1024 * 1024; // 1MB
const SMALL_ALLOCATION_THRESHOLD: usize = 1024; // 1KB

/// A wrapper around a GlobalAlloc implementation (defaults to System)
/// that tracks the total number of bytes currently allocated.
///
/// This struct itself doesn't hold the count; it updates the static
/// `ALLOCATED_BYTES` counter.
#[derive(Debug, Default, Clone, Copy)]
pub struct SaaTrackingAllocator<A = System> {
    inner: A,
}

impl<A> SaaTrackingAllocator<A> {
    /// Creates a new tracking allocator wrapping the given inner allocator.
    /// ## Arguments
    /// * `inner` - The inner allocator to wrap.
    /// ## Returns
    /// A new instance of `SaaTrackingAllocator` wrapping the provided allocator.
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for SaaTrackingAllocator<A> {
    /// Allocates memory of the specified layout and tracks the total allocated bytes.
    /// ## Arguments
    /// * `layout` - The layout of the memory to allocate.
    /// ## Returns
    /// A pointer to the allocated memory, or null if allocation fails.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            let size = layout.size();

            // Track current allocated bytes
            let result =
                ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(size)
                });

            if let Ok(current_total) = result {
                // Update peak usage
                let new_total = current_total + size;
                PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);

                // Track allocation statistics
                TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                BYTES_ALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);

                // Track size categories
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

    /// Deallocates memory of the specified layout and decreases the total allocated bytes.
    /// ## Arguments
    /// * `ptr` - Pointer to the memory to deallocate.
    /// * `layout` - The layout of the memory to deallocate.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();

        // Decrease the counter before deallocating.
        let result =
            ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_sub(size)
            });

        if result.is_err() {
            log::error!("Memory tracking counter overflowed during dealloc! Size: {size}");
        } else {
            // Track deallocation statistics
            TOTAL_DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            BYTES_DEALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);
        }

        unsafe { self.inner.dealloc(ptr, layout) };
    }

    /// Allocates zero-initialized memory of the specified layout and tracks the total allocated bytes.
    /// ## Arguments
    /// * `layout` - The layout of the memory to allocate.
    /// ## Returns
    /// A pointer to the allocated memory, or null if allocation fails.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // Use the inner allocator to allocate zeroed memory.
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };

        if !ptr.is_null() {
            let size = layout.size();

            // Track current allocated bytes
            let result =
                ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(size)
                });

            if let Ok(current_total) = result {
                // Update peak usage
                let new_total = current_total + size;
                PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);

                // Track allocation statistics
                TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                BYTES_ALLOCATED_LIFETIME.fetch_add(size as u64, Ordering::Relaxed);

                // Track size categories
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

    /// Reallocates memory of the specified layout and adjusts the total allocated bytes accordingly.
    /// ## Arguments
    /// * `ptr` - Pointer to the memory to reallocate.
    /// * `layout` - The layout of the memory to reallocate.
    /// * `new_size` - The new size for the memory allocation.
    /// ## Returns
    /// A pointer to the reallocated memory, or null if allocation fails.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_size = layout.size();
        let new_ptr = unsafe { self.inner.realloc(ptr, layout, new_size) };

        if !new_ptr.is_null() {
            // Track reallocation
            TOTAL_REALLOCATIONS.fetch_add(1, Ordering::Relaxed);

            // Adjust counter using checked operations within fetch_update
            let size_diff = new_size as isize - old_size as isize;

            // Use fetch_update to ensure atomicity and avoid potential overflows/underflows.
            let fetch_result = match size_diff.cmp(&0) {
                std::cmp::Ordering::Greater => {
                    // Increase the counter
                    let additional_bytes = size_diff as usize;
                    BYTES_ALLOCATED_LIFETIME.fetch_add(additional_bytes as u64, Ordering::Relaxed);

                    ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_add(additional_bytes)
                    })
                }
                std::cmp::Ordering::Less => {
                    // Decrease the counter
                    let freed_bytes = (-size_diff) as usize;
                    BYTES_DEALLOCATED_LIFETIME.fetch_add(freed_bytes as u64, Ordering::Relaxed);

                    ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_sub(freed_bytes)
                    })
                }
                std::cmp::Ordering::Equal => {
                    Ok(ALLOCATED_BYTES.load(Ordering::Relaxed)) // No change
                }
            };

            // Update peak if we increased
            if size_diff > 0
                && let Ok(new_total) = fetch_result
            {
                PEAK_ALLOCATED_BYTES.fetch_max(new_total as u64, Ordering::Relaxed);
            }

            // Check for overflow or underflow
            if fetch_result.is_err() {
                if size_diff > 0 {
                    log::error!(
                        "Memory tracking counter overflowed during realloc (increase)! Diff: {size_diff}"
                    );
                } else {
                    // Underflow on realloc decrease is serious, likely bug
                    log::error!(
                        "Memory tracking counter underflowed during realloc (decrease)! Diff: {size_diff}"
                    );
                }
            }
        }
        new_ptr
    }
}
