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
//! The trait abstracts the queue implementation: the backend ships a
//! mutex-backed ringbuffer (`DefaultMixBus`, in `khora-infra`), and a
//! lock-free SPSC/SPMC queue can replace it without touching consumers.

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
