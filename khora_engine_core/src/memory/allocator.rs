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
use std::sync::atomic::{AtomicUsize, Ordering};

/// Static atomic counter to hold the currently allocated bytes total.
/// This is kept separate from the allocator struct instance for easier global access.
pub(crate) static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

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
            // Allocation successful, increase the counter.
            // Use fetch_update to ensure atomicity and avoid potential overflows.
            let result =
                ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(layout.size())
                });

            // Overflow
            if result.is_err() {
                log::error!(
                    "Memory tracking counter overflowed during alloc! Size: {}",
                    layout.size()
                );
            }
        }
        ptr
    }

    /// Deallocates memory of the specified layout and decreases the total allocated bytes.
    /// ## Arguments
    /// * `ptr` - Pointer to the memory to deallocate.
    /// * `layout` - The layout of the memory to deallocate.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Decrease the counter before deallocating.
        // Use fetch_update to ensure atomicity and avoid potential underflows.
        let result =
            ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_sub(layout.size())
            });

        // Overflow
        if result.is_err() {
            log::error!(
                "Memory tracking counter overflowed during dealloc! Size: {}",
                layout.size()
            );
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
            // Allocation successful, increase the counter.
            let result =
                ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(layout.size())
                });

            // Overflow
            if result.is_err() {
                log::error!(
                    "Memory tracking counter overflowed during alloc_zeroed! Size: {}",
                    layout.size()
                );
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
            // Adjust counter using checked operations within fetch_update
            let size_diff = new_size as isize - old_size as isize;

            // Use fetch_update to ensure atomicity and avoid potential overflows/underflows.
            let fetch_result = match size_diff.cmp(&0) {
                std::cmp::Ordering::Greater => {
                    // Increase the counter
                    ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_add(size_diff as usize)
                    })
                }
                std::cmp::Ordering::Less => {
                    // Decrease the counter
                    ALLOCATED_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_sub((-size_diff) as usize)
                    })
                }
                std::cmp::Ordering::Equal => {
                    Ok(ALLOCATED_BYTES.load(Ordering::Relaxed)) // No change
                }
            };

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
