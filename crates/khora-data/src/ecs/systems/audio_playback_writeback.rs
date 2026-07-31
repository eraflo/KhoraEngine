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

//! Drains the [`AudioPlaybackWriteback`](crate::flow::AudioPlaybackWriteback)
//! slot the `SpatialMixingLane` wrote into during the CLAD descent and
//! applies the per-source playback-state updates back to `AudioSource`
//! components.
//!
//! This system is the canonical Lane → Deck → DataSystem writeback
//! pattern: the lane is a pure strategy that publishes into the typed
//! `OutputDeck`; the engine threads the same deck through to the
//! `Maintenance`-phase `DataSystem`s; this system drains the slot and
//! mutates the World. No subsystem-specific code lives in the engine.

use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use crate::ecs::{AudioSource, DataSystemRegistration, TickPhase, World};
use crate::flow::AudioPlaybackWriteback;

fn audio_playback_writeback(world: &mut World, _runtime: &Runtime, deck: &mut OutputDeck) {
    let drained = deck.take::<AudioPlaybackWriteback>();
    if drained.updates.is_empty() {
        return;
    }

    // Apply each update to its source. The lane uses the entity's index
    // (its position in the projected `AudioView::sources`) as the
    // writeback key, matching the order of `world.query::<&AudioSource>`
    // — so we match by linear order here too.
    let mut iter = world.query_mut::<&mut AudioSource>();
    for update in drained.updates {
        if let Some(source) = iter.next() {
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
    }
}
