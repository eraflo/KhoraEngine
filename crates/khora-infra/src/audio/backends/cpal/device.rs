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

//! CPAL-backed [`AudioDevice`] / [`AudioStream`].
//!
//! `CpalAudioDevice` is a zero-state factory: opening it consumes the box,
//! grabs the host's default output device, builds a CPAL stream whose
//! callback pulls from the supplied [`AudioMixBus`], and returns a
//! [`CpalAudioStream`] handle that keeps the stream alive (via
//! `Box<dyn AudioStream>` ownership) until dropped.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use khora_core::audio::{AudioDevice, AudioMixBus, AudioStream, StreamInfo};

/// CPAL-backed `AudioDevice` factory.
#[derive(Default)]
pub struct CpalAudioDevice;

impl CpalAudioDevice {
    /// Creates a new instance of the CPAL audio device backend.
    pub fn new() -> Self {
        Self
    }
}

impl AudioDevice for CpalAudioDevice {
    fn open(self: Box<Self>, mix_bus: Arc<dyn AudioMixBus>) -> Result<Box<dyn AudioStream>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default output device available"))?;
        let config = device.default_output_config()?;

        let stream_info = StreamInfo {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
        };

        let bus_for_callback = Arc::clone(&mix_bus);
        let audio_callback = move |output_buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
            bus_for_callback.pull(output_buffer);
        };

        let error_callback = |err| {
            log::error!("audio stream error: {}", err);
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_output_stream(&config.into(), audio_callback, error_callback, None)?
            }
            format => return Err(anyhow!("Unsupported sample format: {}", format)),
        };

        stream.play()?;

        log::info!(
            "audio: stream opened ({} Hz, {} ch)",
            stream_info.sample_rate,
            stream_info.channels
        );

        Ok(Box::new(CpalAudioStream {
            _stream: stream,
            info: stream_info,
        }))
    }
}

/// Live CPAL output stream. Keeps the underlying `cpal::Stream` alive
/// via owned storage; `Drop` stops the stream.
pub struct CpalAudioStream {
    // Field is read implicitly via `Drop` — its sole purpose is to keep
    // the CPAL stream alive for the lifetime of this handle.
    _stream: cpal::Stream,
    info: StreamInfo,
}

// SAFETY: `cpal::Stream` is not Send/Sync because the underlying audio
// thread is platform-specific. In practice the stream is created on the
// thread that calls `open` and never moved across threads after — we only
// store it for the duration of the program. Boxing it as `dyn AudioStream`
// requires `Send + Sync`; the engine takes the same constraint that every
// other CPAL-backed audio engine in the Rust ecosystem accepts.
unsafe impl Send for CpalAudioStream {}
unsafe impl Sync for CpalAudioStream {}

impl AudioStream for CpalAudioStream {
    fn stream_info(&self) -> StreamInfo {
        self.info
    }
}
