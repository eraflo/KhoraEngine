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

//! [`AudioSource`], an entity that emits sound.

use crate::assets::SoundData;
use khora_core::asset::AssetHandle;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Where a playing sound is in its samples, written back from the audio mixing lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
    /// The position in the sample data, in frames.
    ///
    /// An `f64`: in `f32`, a block's advance rounds once a track runs past a
    /// few minutes, and the voice drifts off its rate.
    pub cursor: f64,
}

/// A sound emitted from its entity's position, heard through the `AudioListener`.
#[derive(Debug, Clone, Component)]
#[component(domain = Audio)]
pub struct AudioSource {
    /// The sound to play; not saved.
    #[component(skip)]
    pub handle: AssetHandle<SoundData>,
    /// The gain applied to the sound: `1.0` plays it as recorded, `0.0` silences it.
    pub volume: f32,
    /// Whether the sound starts over when it ends.
    pub looping: bool,
    /// Whether the sound starts playing as soon as the component is added.
    pub autoplay: bool,
    /// Which sources keep a voice when more play than the mixer has voices for.
    ///
    /// A higher priority wins however loud the others are; equal priorities
    /// are ranked by loudness at the listener. Default [`DEFAULT_PRIORITY`] (128).
    pub priority: u8,
    /// The playback position, `None` until the sound plays; owned by the audio engine, not saved.
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
    /// Creates a source of `handle` at full volume that plays as soon as it is added.
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
