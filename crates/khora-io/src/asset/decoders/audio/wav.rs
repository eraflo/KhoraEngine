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

//! `.wav` audio decoder.

use anyhow::{anyhow, Result};
use khora_data::assets::SoundData;
use std::{error::Error, io::Cursor};

use crate::asset::AssetDecoder;

/// Decodes audio from the WAV format using `hound`.
#[derive(Default)]
pub struct WavDecoder;

impl WavDecoder {
    /// Creates a new instance of `WavDecoder`.
    pub fn new() -> Self {
        Self
    }
}

impl AssetDecoder<SoundData> for WavDecoder {
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

#[cfg(test)]
mod tests {
    use super::*;

    // 16-bit, mono, 44100Hz, 4 samples (0.1, -0.1, 0.2, -0.2).
    const TEST_WAV_BYTES: &[u8] = &[
        82, 73, 70, 70, 52, 0, 0, 0, 87, 65, 86, 69, 102, 109, 116, 32, 16, 0, 0, 0, 1, 0, 1, 0,
        68, 172, 0, 0, 136, 88, 1, 0, 2, 0, 16, 0, 100, 97, 116, 97, 8, 0, 0, 0, 0, 12, 204, 251,
        51, 13, 205, 243,
    ];

    #[test]
    fn wav_loader_success() {
        let loader = WavDecoder::new();
        let result = loader.load(TEST_WAV_BYTES);
        assert!(result.is_ok());
        let sound_data = result.unwrap();
        assert_eq!(sound_data.sample_rate, 44100);
        assert_eq!(sound_data.channels, 1);
        assert!(!sound_data.samples.is_empty());
    }

    #[test]
    fn wav_loader_invalid_bytes() {
        let loader = WavDecoder::new();
        let invalid_bytes = &[0, 1, 2, 3, 4];
        assert!(loader.load(invalid_bytes).is_err());
    }
}
