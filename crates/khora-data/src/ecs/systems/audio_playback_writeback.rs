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
//! resource emitted by `SpatialMixingLane` and applies the per-source
//! playback-state updates back to `AudioSource` components.
//!
//! Phase C parks the writeback in a `Resource` rather than a per-tick
//! `OutputDeck` slot because the audio callback runs on a separate
//! thread from the main game loop; the production wiring will push
//! updates from the audio thread into this `Resource`, and the system
//! below drains it on the main thread during `Maintenance`.

use std::sync::{Arc, Mutex};

use khora_core::Runtime;

use crate::ecs::{AudioSource, DataSystemRegistration, TickPhase, World};
use crate::flow::AudioPlaybackWriteback;

/// Lock-protected sink for `AudioPlaybackUpdate`s emitted by the
/// audio callback thread. Lives in `Resources` once the production
/// audio path is wired through; the `audio_playback_writeback`
/// DataSystem drains it on the main thread.
pub type AudioPlaybackSink = Arc<Mutex<AudioPlaybackWriteback>>;

fn audio_playback_writeback(world: &mut World, runtime: &Runtime) {
    let Some(sink) = runtime.resources.get::<AudioPlaybackSink>().cloned() else {
        return;
    };
    let drained = match sink.lock() {
        Ok(mut g) => std::mem::take(&mut *g),
        Err(e) => {
            log::error!("audio_playback_writeback: sink mutex poisoned: {}", e);
            return;
        }
    };
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
