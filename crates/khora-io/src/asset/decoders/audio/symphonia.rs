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
    fn load(&self, bytes: &[u8]) -> Result<SoundData, Box<dyn Error + Send + Sync>> {
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
