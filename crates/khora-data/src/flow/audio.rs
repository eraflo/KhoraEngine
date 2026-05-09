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

//! `AudioFlow` — projects the ECS audio domain into a per-tick
//! [`AudioView`] consumed by `SpatialMixingLane`.
//!
//! Replaces the previous design where the lane queried `World` directly
//! (and mutated `AudioSource.state.cursor` in place from the audio
//! callback thread). Per CLAD, lanes consume Views from the
//! [`LaneBus`](khora_core::lane::LaneBus) and write outputs into the
//! [`OutputDeck`](khora_core::lane::OutputDeck) — they do not query the
//! World.
//!
//! The matching `audio_playback_writeback` `DataSystem`
//! ([`crate::ecs::systems::audio_playback_writeback`], `Maintenance`
//! phase) drains the writeback slot the lane wrote into and applies the
//! new playback states back to the `AudioSource` components.

use khora_core::ecs::entity::EntityId;
use khora_core::math::affine_transform::AffineTransform;
use khora_core::math::Vec3;
use khora_core::Runtime;

use crate::assets::SoundData;
use crate::ecs::{AudioListener, AudioSource, GlobalTransform, PlaybackState, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use khora_core::asset::AssetHandle;

/// Per-source snapshot fed to `SpatialMixingLane::mix`.
#[derive(Debug, Clone)]
pub struct AudioSourceSnapshot {
    /// Origin entity (used to key the writeback).
    pub entity: EntityId,
    /// Sound data the source plays.
    pub handle: AssetHandle<SoundData>,
    /// World-space position of the source.
    pub position: Vec3,
    /// Linear gain.
    pub volume: f32,
    /// Whether the source loops on end.
    pub looping: bool,
    /// Whether the source starts playing on first encounter.
    pub autoplay: bool,
    /// Current playback state, copied from the component each frame.
    /// Mutations made by the lane go through
    /// [`AudioPlaybackWriteback`].
    pub state: Option<PlaybackState>,
}

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

/// View published by [`AudioFlow`] into the
/// [`LaneBus`](khora_core::lane::LaneBus). Carries the listener pose
/// and a snapshot of every active `AudioSource`.
#[derive(Debug, Default, Clone)]
pub struct AudioView {
    /// Number of `AudioSource` components in the world (kept for
    /// backwards-compat telemetry).
    pub source_count: usize,
    /// World-space translation of the first `AudioListener`, if any.
    pub listener_position: Option<Vec3>,
    /// Full affine transform of the first listener (used for spatial
    /// pan / volume math by the mixing lane).
    pub listener_transform: Option<AffineTransform>,
    /// Per-source snapshots consumed by the mixing lane.
    pub sources: Vec<AudioSourceSnapshot>,
}

/// Audio domain Flow.
#[derive(Default)]
pub struct AudioFlow;

impl Flow for AudioFlow {
    type View = AudioView;

    const DOMAIN: SemanticDomain = SemanticDomain::Audio;
    const NAME: &'static str = "audio";

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        let source_count = world.query::<&AudioSource>().count();
        let listener = world
            .query::<(&AudioListener, &GlobalTransform)>()
            .next()
            .map(|(_, t)| t.0);
        let listener_position = listener.map(|t| t.translation());

        // Snapshot every source so the mixing lane never needs to touch
        // the World. The snapshot copy is cheap because `state` is a
        // small `Option<PlaybackState>` and `handle` is an `Arc`.
        let mut sources = Vec::with_capacity(source_count);
        for (entity, (source, transform)) in world
            .query::<(&AudioSource, &GlobalTransform)>()
            .enumerate()
        {
            // CRPECS query yields tuples without entity ids today; the
            // index aligns with `world.iter_entities().filter(...)`. The
            // mixing lane uses `entity` only as a writeback key — a
            // stable index across the same frame is sufficient.
            let _ = entity;
            sources.push(AudioSourceSnapshot {
                entity: EntityId {
                    index: entity as u32,
                    generation: 0,
                },
                handle: source.handle.clone(),
                position: transform.0.translation(),
                volume: source.volume,
                looping: source.looping,
                autoplay: source.autoplay,
                state: source.state.clone(),
            });
        }

        AudioView {
            source_count,
            listener_position,
            listener_transform: listener,
            sources,
        }
    }
}

register_flow!(AudioFlow);
