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

//! Writes the playback states the `SpatialMixingLane` left in the
//! [`AudioPlaybackWriteback`] deck slot back to the `AudioSource` components.

use crate::ecs::SystemContext;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use crate::ecs::{AudioSource, DataSystemRegistration, TickPhase, World};
use crate::flow::AudioPlaybackWriteback;

fn audio_playback_writeback(
    world: &mut World,
    _runtime: &Runtime,
    deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let drained = deck.take::<AudioPlaybackWriteback>();
    if drained.updates.is_empty() {
        return;
    }

    // Each update names its source: a source the View left out (one with no
    // transform) or one despawned since is simply not updated.
    for update in drained.updates {
        if let Some(mut source) = world.get_mut::<AudioSource>(update.entity) {
            source.state = update.new_state;
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "audio_playback_writeback",
        phase: TickPhase::Maintenance,
        run: audio_playback_writeback,
        order_hint: 10, // After physics_world_writeback (default 0).
        runs_after: &[],
        observes: &[],
        role: crate::ecs::Role::Serves(khora_core::audio::AUDIO),
    }
}
