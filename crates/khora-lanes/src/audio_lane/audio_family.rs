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

//! The audio family: the lanes that mix the frame's sound sources, and what
//! every one of them hands on.

use khora_core::agent::gorna::AgentId;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::LaneKind;
use khora_data::flow::AudioPlaybackWriteback;
use khora_macros::LaneOutputs;

/// The audio family. Its output is presentation: a cheaper level mixes fewer
/// voices, never a different playback.
pub struct AudioFamily;

impl LaneFamily for AudioFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "audio";
    const AGENT: AgentId = AgentId::Audio;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = AudioOutputs;
}

/// What every audio lane writes: one playback update per source, drained by
/// the `audio_playback_writeback` DataSystem.
#[derive(LaneOutputs)]
pub struct AudioOutputs {
    /// The playback state of every source after this frame's mix.
    #[deck]
    pub playback: AudioPlaybackWriteback,
}
