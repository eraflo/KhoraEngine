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

//! Defines the `AudioSource` component for emitting sound.

use crate::assets::SoundData;
use khora_core::asset::AssetHandle;
use khora_macros::Component;

/// The internal playback state of an active sound.
/// This will be managed by the `AudioMixingLane`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    /// The current position in the sample data, in samples.
    pub cursor: f32,
}

/// An ECS component that makes an entity an emitter of sound.
#[derive(Debug, Clone, Component)]
pub struct AudioSource {
    /// A handle to the sound data to be played.
    pub handle: AssetHandle<SoundData>,
    /// The volume of the sound, where 1.0 is normal volume.
    pub volume: f32,
    /// Whether the sound should loop back to the beginning when it finishes.
    pub looping: bool,
    /// Whether the sound should start playing automatically when this component is added.
    pub autoplay: bool,
    /// The internal playback state. This should be treated as read-only
    /// by most systems outside of the audio engine itself.
    pub state: Option<PlaybackState>,
}

impl AudioSource {
    /// Creates a new `AudioSource`.
    pub fn new(handle: AssetHandle<SoundData>) -> Self {
        Self {
            handle,
            volume: 1.0,
            looping: false,
            autoplay: true,
            state: None,
        }
    }
}
