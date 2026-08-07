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

//! What a channel costs at frame volumes.
//!
//! The engine sits on one frame-boundary mechanism per event type, never an
//! inner loop: input is a few dozen events a frame, contacts a few hundred at
//! worst, a reload only when somebody saves. These numbers are here so that
//! claim stops being a claim — and so a change that makes the mechanism
//! quadratic shows up as a number rather than as a frame that got slower for
//! reasons nobody could name.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use khora_core::event::{Channel, Cursor, Supersedes, WhenFull};
use std::hint::black_box;

/// A contact: appends, never coalesces. The heavy case.
#[derive(Clone)]
struct Contact {
    a: u64,
    b: u64,
}
impl Supersedes for Contact {}

/// Touches what was read, so the clone is not optimised away and the number
/// measures moving the data rather than discarding it.
fn consume(contacts: &[Contact]) -> u64 {
    contacts
        .iter()
        .map(|c| c.a ^ c.b)
        .fold(0, u64::wrapping_add)
}

/// A recompiled module: coalesces by name, so every send scans.
#[derive(Clone)]
struct Reload {
    module: usize,
}
impl Supersedes for Reload {
    const COALESCES: bool = true;
    fn supersedes(&self, earlier: &Self) -> bool {
        self.module == earlier.module
    }
}

/// A heavy physics frame.
const CONTACTS: u64 = 512;

fn filled(capacity: usize) -> Channel<Contact> {
    let channel = Channel::bounded(capacity, WhenFull::DropOldest);
    for i in 0..CONTACTS {
        channel.send(Contact { a: i, b: i + 1 });
    }
    channel
}

fn sending(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel/send");

    group.bench_function("512 contacts, no coalescing", |b| {
        b.iter_batched(
            || Channel::<Contact>::bounded(1024, WhenFull::DropOldest),
            |channel| {
                for i in 0..CONTACTS {
                    channel.send(Contact { a: i, b: i + 1 });
                }
                black_box(channel.len())
            },
            BatchSize::SmallInput,
        );
    });

    // The reason `COALESCES` is a compile-time constant. This scans; the one
    // above skips the scan entirely. If the two ever converge, the constant
    // stopped doing its job.
    group.bench_function("64 reloads, coalescing", |b| {
        b.iter_batched(
            || Channel::<Reload>::bounded(1024, WhenFull::DropOldest),
            |channel| {
                for i in 0..64 {
                    channel.send(Reload { module: i });
                }
                black_box(channel.len())
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn reading(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel/read");

    group.bench_function("one reader, 512 contacts", |b| {
        let channel = filled(1024);
        b.iter(|| {
            let mut cursor = Cursor::default();
            black_box(consume(&channel.read(&mut cursor)))
        });
    });

    // Three subscribers is the real shape: the shader pump, the script pump and
    // the editor read the same asset stream. Each takes the lock shared, so
    // this should be three times the one-reader cost and not more — a super-
    // linear result would mean they are serialising on it.
    group.bench_function("three readers, 512 contacts", |b| {
        let channel = filled(1024);
        b.iter(|| {
            let mut total = 0;
            for _ in 0..3 {
                let mut cursor = Cursor::default();
                total += consume(&channel.read(&mut cursor));
            }
            black_box(total)
        });
    });

    group.bench_function("nothing new", |b| {
        let channel = filled(1024);
        let mut cursor = Cursor::default();
        channel.read(&mut cursor);
        b.iter(|| black_box(consume(&channel.read(&mut cursor.clone()))));
    });

    group.finish();
}

fn draining(c: &mut Criterion) {
    c.bench_function("channel/drain 512 contacts", |b| {
        b.iter_batched(
            || filled(1024),
            |channel| black_box(consume(&channel.drain())),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, sending, reading, draining);
criterion_main!(benches);
