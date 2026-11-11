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

//! Universal audio decoder using `symphonia`.

use anyhow::{anyhow, Result};
use khora_data::assets::SoundData;
use std::{error::Error, io::Cursor};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};

use crate::asset::AssetDecoder;
use crate::trust::{FileKind, LimitExceeded, Limits, Trust};
use std::path::Path;

/// The bytes one decoded sample fills.
const SAMPLE_BYTES: u64 = std::mem::size_of::<f32>() as u64;

/// Decodes multiple audio formats via `symphonia`.
#[derive(Default)]
pub struct SymphoniaDecoder;

impl SymphoniaDecoder {
    /// Creates a new instance of `SymphoniaDecoder`.
    pub fn new() -> Self {
        Self
    }
}

impl AssetDecoder<SoundData> for SymphoniaDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        crate::trust::magic::is_wav(head)
            || head.starts_with(b"OggS")
            || head.starts_with(b"fLaC")
            || crate::trust::magic::is_mp3(head)
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
        let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());

        let hint = Hint::new();
        let meta_opts: MetadataOptions = Default::default();
        let fmt_opts: FormatOptions = Default::default();
        let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &meta_opts)?;
        let mut format_reader = probed.format;

        let track = format_reader
            .default_track()
            .ok_or_else(|| anyhow!("No default audio track found"))?;

        let track_id = track.id;
        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| anyhow!("Unknown sample rate"))?;
        let channels = track
            .codec_params
            .channels
            .ok_or_else(|| anyhow!("Unknown channel count"))?;

        let dec_opts: DecoderOptions = Default::default();
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &dec_opts)?;

        let mut all_samples = Vec::<f32>::new();
        // A corrupt stream can fail to decode on every packet; warn once on the
        // first failure (with debug! for the rest) and summarise the total after
        // the loop so a bad file cannot flood the log.
        let mut decode_errors: u64 = 0;

        loop {
            match format_reader.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => {
                            let mut sample_buf = SampleBuffer::<f32>::new(
                                decoded.capacity() as u64,
                                *decoded.spec(),
                            );
                            sample_buf.copy_interleaved_ref(decoded);
                            let after = (all_samples.len() + sample_buf.samples().len()) as u64;
                            let decoded_bytes = after.saturating_mul(SAMPLE_BYTES);
                            if decoded_bytes > limits.max_decoded_bytes {
                                return Err(Box::new(LimitExceeded {
                                    what: format!("{after} samples, {decoded_bytes} bytes decoded"),
                                    limit: limits.max_decoded_bytes,
                                }));
                            }
                            all_samples.extend_from_slice(sample_buf.samples());
                        }
                        Err(e) => {
                            if decode_errors == 0 {
                                log::warn!("Audio packet decode error: {e}");
                            } else {
                                log::debug!("Audio packet decode error: {e}");
                            }
                            decode_errors += 1;
                        }
                    }
                }
                Err(symphonia::core::errors::Error::IoError(_)) => {
                    break;
                }
                Err(e) => {
                    return Err(Box::new(e));
                }
            }
        }

        if decode_errors > 1 {
            log::warn!("Audio decoding skipped {decode_errors} corrupt packets in total");
        }

        Ok(SoundData {
            samples: all_samples,
            channels: channels.count() as u16,
            sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// **A sound decoding past its decoded limit is refused by Symphonia**:
    /// 8 000 samples decode to more than 1 000 bytes, so the decode stops
    /// with a `LimitExceeded` naming that limit; within ample limits the
    /// same file decodes whole.
    #[test]
    fn a_sound_decoding_past_its_decoded_limit_is_refused() {
        let file = wav_of(8000);

        let refused = SymphoniaDecoder
            .load_limited(&file, Path::new(""), &decoded_at_most(1000))
            .expect_err("8 000 samples are more than 1 000 decoded bytes");
        let limit =
            exceeded(&*refused).unwrap_or_else(|| panic!("a LimitExceeded, not: {refused}"));
        assert_eq!(limit.limit, 1000);

        let sound = SymphoniaDecoder
            .load_limited(&file, Path::new(""), &decoded_at_most(1 << 20))
            .unwrap_or_else(|e| panic!("the sound decodes within ample limits: {e}"));
        assert_eq!(sound.samples.len(), 8000);
    }
}
