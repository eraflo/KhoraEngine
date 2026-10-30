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

//! The shaper and the rasteriser called from many threads at once.
//!
//! The text backends are reached from concurrent waves (RULES §5): a
//! DataSystem shapes and rasterises while another registers a reloaded font.
//! Each backend holds one leaf lock; one that called out while holding it, or
//! took two locks in conflicting orders, would deadlock here. These tests
//! hammer one shaper and one rasteriser from eight threads in shuffled orders
//! and fail, instead of hanging, if the threads have not all come back within
//! [`WATCHDOG`] (see `graphics/wgpu/device/tests/concurrency.rs` for why a
//! panic and not an abort).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::asset::AssetUUID;
use khora_core::ui::text::{GlyphKey, GlyphRasterizer, ShapeRequest, TextShaper};

use super::{built_in, default_font, key, plex_mono, A, B, H, I, P, SPACE};
use crate::ui::text::{ParleyShaper, SwashRasterizer};

/// How long each thread keeps calling the backends.
const WORK: Duration = Duration::from_millis(300);
/// How long the test waits for every thread before it calls it a deadlock.
const WATCHDOG: Duration = Duration::from_secs(30);
/// Threads calling the backends at once.
const THREADS: usize = 8;

const TEXTS: [&str; 6] = [
    "Hello",
    "Hello world again",
    "a\nb",
    "iiii MMMM",
    "",
    "The quick brown fox jumps over the lazy dog",
];
const SIZES: [f32; 5] = [9.0, 12.5, 16.0, 24.25, 48.0];
const LETTERS: [u16; 5] = [H, A, B, I, P];

// ─── Seeded shuffling, no dependency ────────────────────────────────────────

/// xorshift64: enough to shuffle calls reproducibly per thread.
struct XorShift(u64);

impl XorShift {
    fn seeded(thread: usize) -> Self {
        // Never zero: xorshift stays at zero forever.
        Self(0x9E37_79B9_7F4A_7C15 ^ (thread as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

// ─── The watchdog ───────────────────────────────────────────────────────────

type Worker = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic without a message".to_owned()
    }
}

/// Runs every worker on a thread of its own and waits for all of them for at
/// most [`WATCHDOG`]. Fails with the stuck threads' names on a timeout (they
/// stay detached), and with each worker's error or panic otherwise.
fn run_watched(what: &str, workers: Vec<(String, Worker)>) {
    let (sender, receiver) = mpsc::channel::<(usize, Result<(), String>)>();
    let names: Vec<String> = workers.iter().map(|(name, _)| name.clone()).collect();

    for (index, (name, work)) in workers.into_iter().enumerate() {
        let sender = sender.clone();
        // The handle is dropped: a thread stuck in a deadlock stays detached
        // and cannot keep the test from reporting.
        let _detached = std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(work))
                    .unwrap_or_else(|payload| Err(format!("panicked: {}", panic_message(payload))));
                let _ = sender.send((index, outcome));
            })
            .expect("a test thread spawns");
    }
    drop(sender);

    let deadline = Instant::now() + WATCHDOG;
    let mut finished = vec![false; names.len()];
    let mut failures = Vec::new();
    while finished.iter().any(|done| !done) {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((index, outcome)) => {
                finished[index] = true;
                if let Err(message) = outcome {
                    failures.push(format!("{}: {message}", names[index]));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let stuck: Vec<&str> = names
                    .iter()
                    .zip(&finished)
                    .filter(|(_, done)| !**done)
                    .map(|(name, _)| name.as_str())
                    .collect();
                panic!(
                    "deadlock in {what}: {} of {} threads had not come back {WATCHDOG:?} after \
                     they started, though each stops calling after {WORK:?} — stuck: {stuck:?}. \
                     A text backend holds a lock across a call out, or takes two in \
                     conflicting orders.",
                    stuck.len(),
                    names.len(),
                );
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{what}: a worker thread ended without reporting");
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{what}: calls failed from many threads at once:\n{}",
        failures.join("\n")
    );
}

// ─── The calls ──────────────────────────────────────────────────────────────

/// One shaping call of `rng`'s choosing in the built-in font, or in `font`
/// (registered with Inter or IBM Plex Mono, possibly being replaced).
fn shape_once(shaper: &dyn TextShaper, font: AssetUUID, rng: &mut XorShift) -> Result<(), String> {
    let text = rng.pick(&TEXTS);
    let request = ShapeRequest {
        text,
        font: if rng.below(2) == 0 { built_in() } else { font },
        size_px: rng.pick(&SIZES),
        max_width: if rng.below(2) == 0 { None } else { Some(80.0) },
    };
    let shaped = shaper
        .shape(&request)
        .map_err(|e| format!("shaping {text:?} failed: {e}"))?;
    if !text.is_empty() && shaped.glyphs.is_empty() {
        return Err(format!("{text:?} shaped into no glyph"));
    }
    Ok(())
}

/// One rasterising call of `rng`'s choosing. In the built-in font a letter must
/// give an image and a space none; in `font` (Inter or IBM Plex Mono, possibly
/// being replaced, so the ids name other glyphs) the call only has to return.
fn rasterize_once(
    rasterizer: &dyn GlyphRasterizer,
    font: AssetUUID,
    rng: &mut XorShift,
) -> Result<(), String> {
    let size_px = rng.pick(&SIZES);
    let bin = rng.below(4) as u8;
    let checked = rng.below(2) == 0;
    let font = if checked { built_in() } else { font };
    if rng.below(6) == 0 {
        let image = rasterizer.rasterize(&key(font, SPACE, size_px, bin));
        if checked && image.is_some() {
            return Err("a space rasterised into an image".to_owned());
        }
        return Ok(());
    }
    let glyph = rng.pick(&LETTERS);
    let image = rasterizer.rasterize(&key(font, glyph, size_px, bin));
    if checked && image.is_none() {
        return Err(format!(
            "glyph {glyph} at {size_px} px, bin {bin}, gave nothing"
        ));
    }
    Ok(())
}

/// Inter or IBM Plex Mono, at `rng`'s choosing.
fn some_font(rng: &mut XorShift) -> Arc<[u8]> {
    if rng.below(2) == 0 {
        default_font()
    } else {
        plex_mono()
    }
}

/// Counts every kind of call a worker made, so a test fails if one kind never
/// ran (the mix would then test less than it claims).
fn every_call_ran(calls: &[(&str, usize)]) -> Result<(), String> {
    match calls.iter().find(|(_, count)| *count == 0) {
        Some((name, _)) => Err(format!("{name} never ran in {WORK:?}")),
        None => Ok(()),
    }
}

// ─── The tests ──────────────────────────────────────────────────────────────

/// Eight threads shape texts in the built-in font and in a shared font while
/// they register Inter or IBM Plex Mono over that shared font and under ids of
/// their own. They all come back and every call succeeds.
#[test]
fn the_shaper_called_from_many_threads_at_once_always_returns() {
    let shaper = Arc::new(ParleyShaper::new());
    let shared = AssetUUID::new();
    shaper
        .register_font(shared, default_font())
        .expect("the shared font registers");

    let workers = (0..THREADS)
        .map(|thread| {
            let shaper = Arc::clone(&shaper);
            let work: Worker = Box::new(move || {
                let mut rng = XorShift::seeded(thread);
                let own = AssetUUID::new();
                let mut calls = [("shape", 0), ("register shared", 0), ("register own", 0)];
                let start = Instant::now();
                while start.elapsed() < WORK {
                    match rng.below(8) {
                        0 => {
                            shaper
                                .register_font(shared, some_font(&mut rng))
                                .map_err(|e| format!("registering the shared font failed: {e}"))?;
                            calls[1].1 += 1;
                        }
                        1 => {
                            shaper
                                .register_font(own, some_font(&mut rng))
                                .map_err(|e| format!("registering its own font failed: {e}"))?;
                            calls[2].1 += 1;
                        }
                        _ => {
                            shape_once(&*shaper, shared, &mut rng)?;
                            calls[0].1 += 1;
                        }
                    }
                }
                every_call_ran(&calls)
            });
            (format!("shaper-{thread}"), work)
        })
        .collect();

    run_watched("the shaper", workers);
}

/// Eight threads rasterise letters and spaces of the built-in font and of a
/// shared font at every size and bin while they register Inter or IBM Plex
/// Mono over that shared font and under ids of their own. They all come back
/// and every letter of the built-in font gives an image.
#[test]
fn the_rasterizer_called_from_many_threads_at_once_always_returns() {
    let rasterizer = Arc::new(SwashRasterizer::new());
    let shared = AssetUUID::new();
    rasterizer
        .register_font(shared, default_font())
        .expect("the shared font registers");

    let workers = (0..THREADS)
        .map(|thread| {
            let rasterizer = Arc::clone(&rasterizer);
            let work: Worker = Box::new(move || {
                let mut rng = XorShift::seeded(thread);
                let own = AssetUUID::new();
                let mut calls = [
                    ("rasterize", 0),
                    ("register shared", 0),
                    ("register own", 0),
                ];
                let start = Instant::now();
                while start.elapsed() < WORK {
                    match rng.below(8) {
                        0 => {
                            rasterizer
                                .register_font(shared, some_font(&mut rng))
                                .map_err(|e| format!("registering the shared font failed: {e}"))?;
                            calls[1].1 += 1;
                        }
                        1 => {
                            rasterizer
                                .register_font(own, some_font(&mut rng))
                                .map_err(|e| format!("registering its own font failed: {e}"))?;
                            calls[2].1 += 1;
                        }
                        _ => {
                            rasterize_once(&*rasterizer, shared, &mut rng)?;
                            calls[0].1 += 1;
                        }
                    }
                }
                every_call_ran(&calls)
            });
            (format!("rasterizer-{thread}"), work)
        })
        .collect();

    run_watched("the rasterizer", workers);
}

/// Four threads shape a text and rasterise every glyph of it, as the glyph
/// residency does; four others register a font over the one they use, with
/// both backends, in shuffled orders. They all come back and every call
/// succeeds.
#[test]
fn shaping_and_rasterising_together_from_many_threads_do_not_deadlock() {
    let shaper = Arc::new(ParleyShaper::new());
    let rasterizer = Arc::new(SwashRasterizer::new());
    let shared = AssetUUID::new();
    shaper
        .register_font(shared, default_font())
        .expect("the shared font registers with the shaper");
    rasterizer
        .register_font(shared, default_font())
        .expect("the shared font registers with the rasterizer");

    let workers = (0..THREADS)
        .map(|thread| {
            let shaper = Arc::clone(&shaper);
            let rasterizer = Arc::clone(&rasterizer);
            let work: Worker = if thread % 2 == 0 {
                Box::new(move || {
                    let mut rng = XorShift::seeded(thread);
                    let mut turns = 0usize;
                    let start = Instant::now();
                    while start.elapsed() < WORK {
                        let size_px = rng.pick(&SIZES);
                        let font = if rng.below(2) == 0 {
                            built_in()
                        } else {
                            shared
                        };
                        let text = rng.pick(&TEXTS);
                        let shaped = shaper
                            .shape(&ShapeRequest {
                                text,
                                font,
                                size_px,
                                max_width: None,
                            })
                            .map_err(|e| format!("shaping {text:?} failed: {e}"))?;
                        for g in &shaped.glyphs {
                            let _ = rasterizer.rasterize(&GlyphKey::new(font, g.id, size_px, g.x));
                        }
                        turns += 1;
                    }
                    every_call_ran(&[("shape then rasterize", turns)])
                })
            } else {
                Box::new(move || {
                    let mut rng = XorShift::seeded(thread);
                    let mut registrations = 0usize;
                    let start = Instant::now();
                    while start.elapsed() < WORK {
                        let font = some_font(&mut rng);
                        if rng.below(2) == 0 {
                            shaper
                                .register_font(shared, font.clone())
                                .map_err(|e| format!("the shaper refused a font: {e}"))?;
                            rasterizer
                                .register_font(shared, font)
                                .map_err(|e| format!("the rasterizer refused a font: {e}"))?;
                        } else {
                            rasterizer
                                .register_font(shared, font.clone())
                                .map_err(|e| format!("the rasterizer refused a font: {e}"))?;
                            shaper
                                .register_font(shared, font)
                                .map_err(|e| format!("the shaper refused a font: {e}"))?;
                        }
                        registrations += 1;
                    }
                    every_call_ran(&[("register with both", registrations)])
                })
            };
            (format!("text-{thread}"), work)
        })
        .collect();

    run_watched("the shaper and the rasterizer together", workers);
}
