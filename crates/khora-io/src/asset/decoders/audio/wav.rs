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
use crate::trust::{FileKind, LimitExceeded, Limits, Trust};
use std::path::Path;

/// The bytes one decoded sample fills.
const SAMPLE_BYTES: u64 = std::mem::size_of::<f32>() as u64;

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
    fn sniff(&self, head: &[u8]) -> bool {
        crate::trust::magic::is_wav(head)
    }

    fn limits(&self, trust: Trust) -> Limits {
        Limits::of(FileKind::Audio, trust)
    }

    fn load(&self, bytes: &[u8]) -> Result<SoundData, Box<dyn Error + Send + Sync>> {
        self.load_limited(
            bytes,
            Path::new(""),
            &Limits::of(FileKind::Audio, Trust::Project),
        )
    }

    fn load_limited(
        &self,
        bytes: &[u8],
        _source_path: &Path,
        limits: &Limits,
    ) -> Result<SoundData, Box<dyn Error + Send + Sync>> {
        let cursor = Cursor::new(bytes);
        let mut reader = hound::WavReader::new(cursor)?;

        let spec = reader.spec();
        // The samples the header announces, decoded to `f32`: refused before
        // one is read when they pass the limit.
        let decoded = u64::from(reader.len()).saturating_mul(SAMPLE_BYTES);
        if decoded > limits.max_decoded_bytes {
            return Err(Box::new(LimitExceeded {
                what: format!("{} samples, {decoded} bytes decoded", reader.len()),
                limit: limits.max_decoded_bytes,
            }));
        }

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

    use crate::trust::{LimitExceeded, Limits};
    use std::path::Path;

    /// The `LimitExceeded` an error is, or holds among its sources.
    fn exceeded<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a LimitExceeded> {
        let mut next = Some(error);
        while let Some(error) = next {
            if let Some(limit) = error.downcast_ref::<LimitExceeded>() {
                return Some(limit);
            }
            next = error.source();
        }
        None
    }

    /// A 16-bit mono WAV of `samples` samples, written in memory.
    fn wav_of(samples: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer =
            hound::WavWriter::new(Cursor::new(&mut bytes), spec).expect("a WAV writer");
        for i in 0..samples {
            writer
                .write_sample((i % 100) as i16 * 100)
                .expect("a sample is written");
        }
        writer.finalize().expect("the WAV is finished");
        bytes
    }

    /// Limits that stop nothing but a decode past `max_decoded_bytes`.
    fn decoded_at_most(max_decoded_bytes: u64) -> Limits {
        Limits {
            max_bytes: u64::MAX,
            max_decoded_bytes,
            max_elements: u64::MAX,
            max_dimension: u32::MAX,
        }
    }

    /// **A WAV decoding past its decoded limit is refused**: 1 000 samples
    /// decode to more than 1 000 bytes, so the decode stops with a
    /// `LimitExceeded` naming that limit; within ample limits the same file
    /// decodes whole.
    #[test]
    fn a_wav_decoding_past_its_decoded_limit_is_refused() {
        let file = wav_of(1000);

        let refused = WavDecoder
            .load_limited(&file, Path::new(""), &decoded_at_most(1000))
            .expect_err("1 000 samples are more than 1 000 decoded bytes");
        let limit =
            exceeded(&*refused).unwrap_or_else(|| panic!("a LimitExceeded, not: {refused}"));
        assert_eq!(limit.limit, 1000);

        let sound = WavDecoder
            .load_limited(&file, Path::new(""), &decoded_at_most(1 << 20))
            .unwrap_or_else(|e| panic!("the WAV decodes within ample limits: {e}"));
        assert_eq!(sound.samples.len(), 1000);
    }
}
