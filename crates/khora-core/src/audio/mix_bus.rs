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

//! Cross-thread audio mix bus.
//!
//! Bridge between audio-rendering lanes (main thread, ~60 Hz) and the
//! audio backend's hardware callback (RT thread, ~kHz). Contributors call
//! [`AudioMixBus::write_block`] to push pre-mixed PCM frames; the backend
//! callback calls [`AudioMixBus::pull`] to drain the next N samples into
//! the hardware buffer.
//!
//! The trait abstracts the queue implementation so the v1 ships a simple
//! mutex-backed ringbuffer ([`DefaultMixBus`]) and a future PR can swap in
//! a lock-free SPSC/SPMC queue without touching consumers.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::device::StreamInfo;

/// The contract between audio-producing lanes and the audio backend.
///
/// All methods are `&self` so the bus can sit inside an `Arc` shared by
/// multiple lanes (writers) and the backend's callback (single reader).
pub trait AudioMixBus: Send + Sync {
    /// Channel count and sample rate negotiated with the device.
    ///
    /// Lanes use this to size their writes (frames × channels) and to
    /// avoid sample-rate conversion mismatches.
    fn stream_info(&self) -> StreamInfo;

    /// Push interleaved PCM samples produced by an audio lane.
    ///
    /// `samples.len()` must be a multiple of `stream_info().channels`.
    /// Implementations may drop the oldest data on overflow — audio is
    /// real-time and stale samples are worse than silence.
    fn write_block(&self, samples: &[f32]);

    /// Drain the next `out.len()` samples into the hardware output
    /// buffer. Called from the audio backend's callback thread.
    ///
    /// Underrun (queue shorter than `out.len()`) is filled with silence
    /// (`0.0`) — never blocks, never allocates.
    fn pull(&self, out: &mut [f32]);
}

/// Mutex-backed ringbuffer impl of [`AudioMixBus`].
///
/// Lives in `khora-core` so apps and tests can construct one without
/// pulling `khora-infra`. Performance is adequate for early development;
/// migration to a lock-free queue is a follow-up PR.
pub struct DefaultMixBus {
    info: StreamInfo,
    capacity_samples: usize,
    queue: Mutex<VecDeque<f32>>,
}

impl DefaultMixBus {
    /// `capacity_frames` is the high-water mark in *frames* (not samples).
    /// Samples beyond that are dropped from the head — newest wins.
    #[must_use]
    pub fn new(info: StreamInfo, capacity_frames: usize) -> Self {
        let capacity_samples = capacity_frames * info.channels as usize;
        Self {
            info,
            capacity_samples,
            queue: Mutex::new(VecDeque::with_capacity(capacity_samples)),
        }
    }
}

impl AudioMixBus for DefaultMixBus {
    fn stream_info(&self) -> StreamInfo {
        self.info
    }

    fn write_block(&self, samples: &[f32]) {
        let Ok(mut q) = self.queue.lock() else {
            return;
        };
        q.extend(samples.iter().copied());
        // Trim from the head once we exceed capacity — keep the newest
        // data; old samples are stale and worse than silence in real time.
        while q.len() > self.capacity_samples {
            q.pop_front();
        }
    }

    fn pull(&self, out: &mut [f32]) {
        let Ok(mut q) = self.queue.lock() else {
            out.fill(0.0);
            return;
        };
        let take = out.len().min(q.len());
        for slot in out.iter_mut().take(take) {
            // SAFETY (logical): `take <= q.len()` so pop_front is non-None.
            *slot = q.pop_front().unwrap_or(0.0);
        }
        if take < out.len() {
            for slot in out.iter_mut().skip(take) {
                *slot = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> StreamInfo {
        StreamInfo {
            channels: 2,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn write_then_pull_roundtrips_same_sequence() {
        let bus = DefaultMixBus::new(info(), 1024);
        let written: Vec<f32> = (0..16).map(|i| i as f32 * 0.1).collect();
        bus.write_block(&written);

        let mut out = vec![0.0_f32; 16];
        bus.pull(&mut out);
        assert_eq!(out, written);
    }

    #[test]
    fn pull_under_run_fills_silence() {
        let bus = DefaultMixBus::new(info(), 1024);
        bus.write_block(&[0.5, 0.5]);
        let mut out = vec![1.0_f32; 8];
        bus.pull(&mut out);
        assert_eq!(out[0..2], [0.5, 0.5]);
        assert!(out[2..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn write_overflow_drops_oldest_samples() {
        let bus = DefaultMixBus::new(info(), 4); // 4 frames * 2 ch = 8 samples
        let burst: Vec<f32> = (0..12).map(|i| i as f32).collect();
        bus.write_block(&burst);

        let mut out = vec![0.0_f32; 8];
        bus.pull(&mut out);
        // Newest 8 samples kept (4..=11), oldest 4 dropped.
        assert_eq!(out, vec![4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
    }

    #[test]
    fn stream_info_is_what_was_passed_in() {
        let bus = DefaultMixBus::new(info(), 1024);
        let si = bus.stream_info();
        assert_eq!(si.channels, 2);
        assert_eq!(si.sample_rate, 48_000);
    }
}
