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
        Self::default()
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