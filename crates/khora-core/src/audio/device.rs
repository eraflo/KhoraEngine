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

//! Defines the abstract `AudioDevice` trait.

use anyhow::Result;

/// A struct providing information about the audio stream.
#[derive(Debug, Clone, Copy)]
pub struct StreamInfo {
    /// The number of channels (e.g., 2 for stereo).
    pub channels: u16,
    /// The number of samples per second (e.g., 44100 Hz).
    pub sample_rate: u32,
}

/// The abstract contract for a hardware audio device backend.
///
/// This trait is the boundary between the engine's audio logic (mixing, spatialization)
/// and the platform-specific infrastructure that actually communicates with the sound card.
/// Its design is callback-driven: the engine provides a function that the backend
/// calls whenever it needs more audio data.
pub trait AudioDevice: Send + Sync {
    /// Initializes and starts the audio stream.
    ///
    /// This method consumes the `AudioDevice` as it typically runs for the lifetime
    /// of the application.
    ///
    /// # Arguments
    ///
    /// * `on_mix_needed`: A closure that will be called repeatedly by the audio backend
    ///   on a dedicated audio thread. This closure is responsible for filling the
    ///   provided `output_buffer` with the next chunk of audio samples. The samples
    ///   should be interleaved for multi-channel audio (e.g., `[L, R, L, R, ...]`).
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure in initializing the audio stream.
    fn start(
        self: Box<Self>,
        on_mix_needed: Box<dyn FnMut(&mut [f32], &StreamInfo) + Send>,
    ) -> Result<()>;
}