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

//! What a steady frame of mixing allocates.
//!
//! The binary's allocator notes the largest allocation made on a thread while
//! that thread measures a run ([`measuring`]), and nothing else: the other
//! tests of this binary, on their own threads, are never counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use khora_core::math::Vec3;

use super::fixtures::{measuring, sine, view_of, Mixer, Voice, ALL_VOICES};

/// The system allocator, noting the largest allocation of a measured run.
struct LargestWhileMeasuring;

/// The largest allocation, in bytes, made by a measured run.
static LARGEST: AtomicUsize = AtomicUsize::new(0);

fn note(size: usize) {
    if measuring() {
        LARGEST.fetch_max(size, Ordering::SeqCst);
    }
}

// SAFETY: every call is forwarded unchanged to `System`, which upholds the
// `GlobalAlloc` contract; `note` only reads a thread-local flag and updates an
// atomic, and allocates nothing.
unsafe impl GlobalAlloc for LargestWhileMeasuring {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: forwarded with the caller's layout, as `System` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: forwarded with the caller's layout, as `System` requires.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from this allocator, which forwards to
        // `System`, with this same layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        // SAFETY: `pointer` came from this allocator with `layout`, as
        // `System::realloc` requires; `new_size` is the caller's.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: LargestWhileMeasuring = LargestWhileMeasuring;

/// The block the lane mixes into is kept across frames: once the first frame
/// has run, a frame allocates nothing as large as the block it pushes to the
/// mix bus.
#[test]
fn a_steady_frame_allocates_no_mix_buffer() {
    let voices = [Voice::playing(&sine(48_000, 48_000), 1.0).at(Vec3::new(1.0, 0.0, 0.0))];
    let view = view_of(Some(Vec3::ZERO), &voices);
    let mut mixer = Mixer::stereo();
    let first = mixer.mix(ALL_VOICES, view.clone());
    first.recorded();
    let block_bytes = first.block.len() * std::mem::size_of::<f32>();

    LARGEST.store(0, Ordering::SeqCst);
    let steady = mixer.mix_measured(ALL_VOICES, view);
    let largest = LARGEST.load(Ordering::SeqCst);

    steady.recorded();
    assert!(
        largest < block_bytes,
        "a steady frame allocated {largest} bytes at once; the block it mixes is {block_bytes} bytes"
    );
}
