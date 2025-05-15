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

/// Gets the total number of bytes currently allocated via the global allocator.
/// This function provides a way to monitor memory usage in the application.
/// ## Returns
/// The total number of bytes currently allocated.
pub fn get_currently_allocated_bytes() -> usize {
    allocator::ALLOCATED_BYTES.load(Ordering::Relaxed)
}
