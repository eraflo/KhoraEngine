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

//! Implements an asset loader for `.wav` audio files.

use crate::asset_lane::loading::AssetLoaderLane;
use anyhow::{anyhow, Result};
use khora_data::assets::SoundData;
use std::{error::Error, io::Cursor};

/// An `AssetLoaderLane` that decodes audio data from the WAV format.
#[derive(Default)]
pub struct WavLoaderLane;

impl WavLoaderLane {
    /// Creates a new instance of `WavLoaderLane`.
    pub fn new() -> Self {
        Self
    }
}

impl AssetLoaderLane<SoundData> for WavLoaderLane {
    /// Parses a byte slice representing a `.wav` file into a `SoundData` asset.
    fn load(&self, bytes: &[u8]) -> Result<SoundData, Box<dyn Error + Send + Sync>> {
        let cursor = Cursor::new(bytes);
        let mut reader = hound::WavReader::new(cursor)?;

        let spec = reader.spec();

        let samples: Result<Vec<f32>, _> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect(),
            hound::SampleFormat::Int => {
                let max_value = (1 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|s| s as f32 / max_value))
                    .collect()
            }
        };

        let samples = samples.map_err(|e| anyhow!("Failed to parse WAV samples: {}", e))?;

        Ok(SoundData {
            samples,
            channels: spec.channels,
            sample_rate: spec.sample_rate,
        })
    }
}

impl khora_core::lane::Lane for WavLoaderLane {
    fn strategy_name(&self) -> &'static str {
        "WavLoader"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Asset
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A WAV file 16-bit, mono, 44100Hz, containing 4 samples (0.1, -0.1, 0.2, -0.2).
    const TEST_WAV_BYTES: &[u8] = &[
        82, 73, 70, 70, 52, 0, 0, 0, 87, 65, 86, 69, 102, 109, 116, 32, 16, 0, 0, 0, 1, 0, 1, 0,
        68, 172, 0, 0, 136, 88, 1, 0, 2, 0, 16, 0, 100, 97, 116, 97, 8, 0, 0, 0, 0, 12, 204, 251,
        51, 13, 205, 243,
    ];

    #[test]
    fn test_wav_loader_success() {
        let loader = WavLoaderLane::new();
        let result = loader.load(TEST_WAV_BYTES);

        assert!(result.is_ok(), "The WAV loading should not fail");
        let sound_data = result.unwrap();

        assert_eq!(
            sound_data.sample_rate, 44100,
            "The sample rate is incorrect"
        );
        assert_eq!(
            sound_data.channels, 1,
            "The number of channels is incorrect"
        );
        assert!(!sound_data.samples.is_empty(), "No samples were loaded");
    }

    #[test]
    fn test_wav_loader_invalid_bytes() {
        let loader = WavLoaderLane::new();
        let invalid_bytes = &[0, 1, 2, 3, 4];
        let result = loader.load(invalid_bytes);

        assert!(result.is_err(), "The loading of invalid bytes should fail");
    }
}
