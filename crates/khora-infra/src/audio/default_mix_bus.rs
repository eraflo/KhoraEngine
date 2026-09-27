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

//! The default [`AudioMixBus`]: a mutex-backed ringbuffer between the audio
//! lanes and the backend's hardware callback.

use std::collections::VecDeque;
use std::sync::Mutex;

use khora_core::audio::{AudioMixBus, StreamInfo};

/// Mutex-backed ringbuffer impl of [`AudioMixBus`].
///
/// Performance is adequate for now; a lock-free queue can replace it behind
/// the same trait.
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
