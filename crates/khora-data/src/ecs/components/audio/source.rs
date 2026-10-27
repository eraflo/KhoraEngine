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
use serde::{Deserialize, Serialize};

/// The internal playback state of an active sound.
/// This will be managed by the `AudioMixingLane`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
    /// The current position in the sample data, in frames. An `f64`: an
    /// `f32` position rounds a block's advance once a track runs past a few
    /// minutes, and the voice drifts off its rate.
    pub cursor: f64,
}

/// An ECS component that makes an entity an emitter of sound.
#[derive(Debug, Clone, Component)]
#[component(domain = Audio)]
pub struct AudioSource {
    /// A handle to the sound data to be played.
    #[component(skip)]
    pub handle: AssetHandle<SoundData>,
    /// The volume of the sound, where 1.0 is normal volume.
    pub volume: f32,
    /// Whether the sound should loop back to the beginning when it finishes.
    pub looping: bool,
    /// Whether the sound should start playing automatically when this component is added.
    pub autoplay: bool,
    /// How much the source matters when more sources play than the mixer has
    /// voices for: a higher priority keeps its voice before any lower one,
    /// however loud. Sources of equal priority are ranked by how loud they
    /// are at the listener. The default, 128, sits in the middle.
    pub priority: u8,
    /// The internal playback state. This should be treated as read-only
    /// by most systems outside of the audio engine itself.
    #[component(skip)]
    pub state: Option<PlaybackState>,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            handle: AssetHandle::dangling(),
            volume: 1.0,
            looping: false,
            autoplay: false,
            priority: DEFAULT_PRIORITY,
            state: None,
        }
    }
}

/// The priority a source has unless its author gave it another.
pub const DEFAULT_PRIORITY: u8 = 128;

impl AudioSource {
    /// Creates a new `AudioSource`.
    pub fn new(handle: AssetHandle<SoundData>) -> Self {
        Self {
            handle,
            volume: 1.0,
            looping: false,
            autoplay: true,
            priority: DEFAULT_PRIORITY,
            state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source an author gave no priority sits in the middle of the range,
    /// whichever way it was made.
    #[test]
    fn a_source_has_the_default_priority_unless_given_one() {
        assert_eq!(AudioSource::default().priority, DEFAULT_PRIORITY);
        assert_eq!(
            AudioSource::new(AssetHandle::dangling()).priority,
            DEFAULT_PRIORITY
        );
    }
}
