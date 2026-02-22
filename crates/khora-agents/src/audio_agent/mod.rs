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

//! The Intelligent Subsystem Agent responsible for managing the audio system.

use anyhow::Result;
use khora_core::audio::device::AudioDevice;
use khora_data::ecs::World;
use khora_lanes::audio_lane::SpatialMixingLane;
use std::sync::{Arc, RwLock};

/// The ISA that orchestrates the entire audio system.
pub struct AudioAgent {
    /// The audio device used for playback.
    device: Option<Box<dyn AudioDevice>>,
    /// The audio mixing lane responsible for mixing audio sources.
    mixing_lane: Arc<SpatialMixingLane>,
    /// A thread-safe, shareable reference to the ECS `World`.
    world: Arc<RwLock<World>>,
}

impl AudioAgent {
    /// Creates a new `AudioAgent`.
    ///
    /// # Arguments
    /// * `world`: A thread-safe, shareable reference to the ECS `World`.
    /// * `device`: A boxed, concrete implementation of the `AudioDevice` trait.
    pub fn new(world: Arc<RwLock<World>>, device: Box<dyn AudioDevice>) -> Self {
        Self {
            device: Some(device),
            mixing_lane: Arc::new(SpatialMixingLane::new()),
            world,
        }
    }

    /// Initializes the audio backend and starts the audio stream.
    /// This method consumes the device, so it can only be called once.
    pub fn start(&mut self) -> Result<()> {
        // Take ownership of the device. This ensures `start` can't be called twice.
        if let Some(device_boxed) = self.device.take() {
            let mixing_lane = self.mixing_lane.clone();
            let world = self.world.clone();

            let on_mix_needed = Box::new(
                move |output_buffer: &mut [f32],
                      stream_info: &khora_core::audio::device::StreamInfo| {
                    if let Ok(mut world) = world.write() {
                        mixing_lane.mix(&mut world, output_buffer, stream_info);
                    }
                },
            );

            // Start the device stream.
            device_boxed.start(on_mix_needed)
        } else {
            // The device has already been started or was never provided.
            Ok(())
        }
    }
}
