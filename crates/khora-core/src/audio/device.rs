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

//! Defines the abstract [`AudioDevice`] / [`AudioStream`] traits.
//!
//! An `AudioDevice` is a *factory* for an output stream: call
//! [`AudioDevice::open`] once and you get a long-lived [`AudioStream`]
//! handle whose `Drop` stops the stream. The device itself is consumed
//! during opening — it has no state worth keeping after the stream is
//! live.
//!
//! The backend's hardware callback drains samples from the
//! [`AudioMixBus`](super::mix_bus::AudioMixBus) provided to `open`. Audio
//! lanes write into the same bus from the main thread; the bus is the
//! sole synchronisation boundary between the two worlds.

use std::sync::Arc;

use anyhow::Result;

use super::mix_bus::AudioMixBus;

/// A struct providing information about the audio stream.
#[derive(Debug, Clone, Copy)]
pub struct StreamInfo {
    /// The number of channels (e.g., 2 for stereo).
    pub channels: u16,
    /// The number of samples per second (e.g., 44100 Hz).
    pub sample_rate: u32,
}

/// Factory for an audio output stream.
///
/// The device is consumed during [`AudioDevice::open`] — backends store
/// their hardware handles inside the returned [`AudioStream`].
pub trait AudioDevice: Send + Sync {
    /// Opens the output stream. The backend's audio callback will pull
    /// samples from `mix_bus` on a dedicated real-time thread.
    ///
    /// The returned handle keeps the stream alive for as long as it
    /// exists. Dropping the handle stops the stream.
    fn open(self: Box<Self>, mix_bus: Arc<dyn AudioMixBus>) -> Result<Box<dyn AudioStream>>;
}

/// Live audio stream handle. Drop = stop.
pub trait AudioStream: Send + Sync {
    /// Channel count and sample rate of the stream as actually opened by
    /// the backend (may differ from the bus's nominal info if the
    /// hardware imposed a different format).
    fn stream_info(&self) -> StreamInfo;
}
