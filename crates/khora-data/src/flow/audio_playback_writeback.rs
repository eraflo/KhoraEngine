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

//! `AudioPlaybackWriteback` — the playback updates the mixing lane hands back
//! for the `audio_playback_writeback` DataSystem to apply.

use khora_core::ecs::entity::EntityId;

use crate::ecs::PlaybackState;

/// Update emitted by `SpatialMixingLane` for a single source — applied
/// back to its `AudioSource` component by the
/// `audio_playback_writeback` DataSystem.
#[derive(Debug, Clone)]
pub struct AudioPlaybackUpdate {
    /// Target entity.
    pub entity: EntityId,
    /// New playback state. `None` means "stopped" (cursor reset).
    pub new_state: Option<PlaybackState>,
}

/// Slot type written into [`OutputDeck`](khora_core::lane::OutputDeck)
/// by `SpatialMixingLane`. Drained in `Maintenance` by the
/// `audio_playback_writeback` DataSystem.
#[derive(Debug, Default, Clone)]
pub struct AudioPlaybackWriteback {
    /// One update per source touched this frame.
    pub updates: Vec<AudioPlaybackUpdate>,
}
