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

//! What reading a hostile compact payload costs in memory.
//!
//! Its own test binary: it counts every allocation the process makes, and in
//! a shared binary the other tests' allocations would be counted with it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use khora_data::scene::record::Record;
use khora_data::scene::{CompactEncoding, SceneEncoding};

/// The system allocator, keeping the most memory ever held at once.
struct Peak;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to `System`, which upholds the
// `GlobalAlloc` contract; the counters only observe the sizes.
unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded with the caller's layout, as `System` requires.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above with this same layout.
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// A compact payload of one page, one row, one value: `levels` lists nested
/// in one another, each claiming `claim` items, then `claim` unit values.
///
/// Every claim is backed by the bytes left after it — each count passes the
/// reader's room check — yet the file holds one list's worth of items, not
/// `levels` lists' worth.
fn nested_claims(levels: usize, claim: u64) -> Vec<u8> {
    const SEQ: u8 = 12;
    const UNIT: u8 = 0;
    let mut out = vec![1]; // the layout version
    varint(&mut out, 1); // one symbol
    varint(&mut out, 1);
    out.push(b'C');
    varint(&mut out, 0); // no shapes
    varint(&mut out, 0); // no entities
    varint(&mut out, 1); // one page
    varint(&mut out, 1); // one component
    varint(&mut out, 0); // named `C`
    varint(&mut out, 1); // one row
    out.extend_from_slice(&1u64.to_le_bytes());
    for _ in 0..levels {
        out.push(SEQ);
        varint(&mut out, claim);
    }
    out.extend(std::iter::repeat_n(UNIT, claim as usize));
    out
}

/// A compact payload is refused by what it holds, not by what it claims:
/// lists nested in one another, each claiming every byte left, must not make
/// a small file reserve room for each claim at every level.
///
/// Each nested list reserves `left` records up front (`Vec::with_capacity`
/// in the compact reader), and the depth limit lets that repeat over a
/// hundred times: a 16 KiB file reserves well over a hundred megabytes before
/// it is refused, and a file of a few megabytes asks for more memory than a
/// machine has — which aborts the process instead of failing the load.
#[test]
fn nested_list_claims_do_not_multiply_into_memory() {
    let claim = 16 * 1024;
    let payload = nested_claims(120, claim);
    // Twice what a record per byte of file costs — a flat list of units
    // holds that much, and a file cannot back more values than its bytes.
    let allowed = payload.len() * std::mem::size_of::<Record>() * 2;

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let decoded = CompactEncoding.decode(&payload);
    let peak = PEAK.load(Ordering::Relaxed) - before;

    assert!(decoded.is_err(), "a payload that ends early is refused");
    assert!(
        peak <= allowed,
        "reading {} bytes held {peak} bytes at once ({}x the file); at most {allowed} expected",
        payload.len(),
        peak / payload.len()
    );
}
