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

//! Defines the core asset type for audio data.

use khora_core::asset::Asset;

/// Represents a sound asset, decoded and ready for playback.
///
/// This struct holds audio data in a normalized, interleaved `f32` format,
/// which is the standard for high-quality audio processing pipelines.
/// The `AssetAgent` will manage instances of this type in its cache.
#[derive(Debug, Clone)]
pub struct SoundData {
    /// The raw, interleaved audio samples.
    /// For stereo, samples are ordered `[L, R, L, R, ...]`.
    /// Values are expected to be in the range `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// The number of channels in the audio data (e.g., 1 for mono, 2 for stereo).
    pub channels: u16,
    /// The number of samples per second (e.g., 44100 Hz).
    pub sample_rate: u32,
}

// By implementing `Asset`, `SoundData` can be used with `AssetHandle<T>`
// and managed by the `AssetAgent`.
impl Asset for SoundData {}
