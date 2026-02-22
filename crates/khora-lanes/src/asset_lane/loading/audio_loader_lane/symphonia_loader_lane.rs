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

//! Implements a universal asset loader for audio formats using the `symphonia` library.

use crate::asset_lane::loading::AssetLoaderLane;
use anyhow::{anyhow, Result};
use khora_data::assets::SoundData;
use std::{error::Error, io::Cursor};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};

/// An `AssetLoaderLane` that uses `symphonia` to decode multiple audio formats.
#[derive(Default)]
pub struct SymphoniaLoaderLane;

impl SymphoniaLoaderLane {
    /// Creates a new instance of `SymphoniaLoaderLane`.
    pub fn new() -> Self {
        Self
    }
}

impl AssetLoaderLane<SoundData> for SymphoniaLoaderLane {
    fn load(&self, bytes: &[u8]) -> Result<SoundData, Box<dyn Error + Send + Sync>> {
        // 1. Create a media source stream from the in-memory byte slice.
        let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());

        // 2. Probe for the format. A hint can be useful but is not required.
        let hint = Hint::new();
        let meta_opts: MetadataOptions = Default::default();
        let fmt_opts: FormatOptions = Default::default();
        let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &meta_opts)?;
        let mut format_reader = probed.format;

        // 3. Find the default audio track.
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

        // 4. Create a decoder for the track.
        let dec_opts: DecoderOptions = Default::default();
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &dec_opts)?;

        // 5. Decode all packets and collect the samples.
        let mut all_samples = Vec::<f32>::new();

        loop {
            match format_reader.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => {
                            // Symphonia gives us samples in planes (e.g., LLL..., RRR...).
                            // We need to convert and interleave them into our LRLR... format.
                            let mut sample_buf = SampleBuffer::<f32>::new(
                                decoded.capacity() as u64,
                                *decoded.spec(),
                            );
                            sample_buf.copy_interleaved_ref(decoded);
                            all_samples.extend_from_slice(sample_buf.samples());
                        }
                        Err(e) => {
                            // A decode error is not fatal, just log and continue.
                            eprintln!("Decode error: {}", e);
                        }
                    }
                }
                // End of stream
                Err(symphonia::core::errors::Error::IoError(_)) => {
                    break;
                }
                Err(e) => {
                    return Err(Box::new(e));
                }
            }
        }

        Ok(SoundData {
            samples: all_samples,
            channels: channels.count() as u16,
            sample_rate,
        })
    }
}

impl khora_core::lane::Lane for SymphoniaLoaderLane {
    fn strategy_name(&self) -> &'static str {
        "SymphoniaLoader"
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
