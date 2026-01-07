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

//! Contains the `CpalAudioDevice` struct.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use khora_core::audio::device::{AudioDevice, StreamInfo};

/// An `AudioDevice` implementation that uses the host's default audio output device via CPAL.
#[derive(Default)]
pub struct CpalAudioDevice;

impl CpalAudioDevice {
    /// Creates a new instance of the CPAL audio device backend.
    pub fn new() -> Self {
        Self
    }
}

impl AudioDevice for CpalAudioDevice {
    fn start(
        self: Box<Self>,
        mut on_mix_needed: Box<dyn FnMut(&mut [f32], &StreamInfo) + Send>,
    ) -> Result<()> {
        // Set up the CPAL audio stream.
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default output device available"))?;
        let config = device.default_output_config()?;

        let stream_info = StreamInfo {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
        };

        let audio_callback = move |output_buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
            on_mix_needed(output_buffer, &stream_info);
        };

        let error_callback = |err| {
            eprintln!("An error occurred on the audio stream: {}", err);
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_output_stream(&config.into(), audio_callback, error_callback, None)?
            }
            format => return Err(anyhow!("Unsupported sample format: {}", format)),
        };

        stream.play()?;

        // Detach the stream to keep it running for the lifetime of the application.
        std::mem::forget(stream);

        Ok(())
    }
}
