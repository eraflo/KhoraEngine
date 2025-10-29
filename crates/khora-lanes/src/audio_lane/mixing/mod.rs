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

//! Groups different audio mixing lanes.

mod spatial_mixing_lane;

pub use spatial_mixing_lane::*;

/// A trait defining the behavior of an audio mixing lane.
pub trait AudioMixingLane: Send + Sync {
    /// Mixes audio into the provided output buffer based on the current state of the ECS `World`.
    ///
    /// # Arguments
    /// * `world`: A reference to the ECS `World` containing audio sources and their states.
    /// * `output_buffer`: The buffer to write mixed audio samples into.
    /// * `stream_info`: Information about the audio stream (e.g., sample rate, channels).
    fn mix(
        &self,
        world: &mut khora_data::ecs::World,
        output_buffer: &mut [f32],
        stream_info: &khora_core::audio::device::StreamInfo,
    );
}
