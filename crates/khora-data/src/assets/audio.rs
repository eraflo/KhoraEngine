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

//! The decoded sound asset.

use khora_core::asset::Asset;

/// A sound decoded to interleaved `f32` samples, ready for playback.
#[derive(Debug, Clone, Default)]
pub struct SoundData {
    /// The interleaved samples, in `[-1.0, 1.0]`.
    ///
    /// For stereo, samples are ordered `[L, R, L, R, ...]`.
    pub samples: Vec<f32>,
    /// The number of channels in the audio data (e.g., 1 for mono, 2 for stereo).
    pub channels: u16,
    /// The number of samples per second (e.g., 44100 Hz).
    pub sample_rate: u32,
}

impl Asset for SoundData {}
