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

use std::collections::HashMap;

use khora_core::control::gorna::ResourceBudget;
use khora_core::ecs::entity::EntityId;
use khora_core::math::affine_transform::AffineTransform;
use khora_core::math::Vec3;
use khora_core::Runtime;

use crate::assets::SoundData;
use crate::ecs::{AudioListener, AudioSource, GlobalTransform, PlaybackState, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use khora_core::asset::AssetHandle;

/// Distance beyond which an `AudioSource` is detached from the World.
/// At this distance the source contributes essentially nothing to the
/// final mix, so processing it wastes CPU. Mirrors the AGDF pattern in
/// [`super::physics::PhysicsFlow`].
const AUDIO_DETACH_RADIUS: f32 = 100.0;

/// Distance below which a previously-stashed `AudioSource` is restored.
/// Smaller than [`AUDIO_DETACH_RADIUS`] to provide hysteresis (avoids
/// thrashing when an entity oscillates around the boundary).
const AUDIO_REATTACH_RADIUS: f32 = 80.0;

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
pub struct AudioFlow {
    /// Stashed `AudioSource` components, keyed by entity. Restored when
    /// the entity comes back inside [`AUDIO_REATTACH_RADIUS`] of the
    /// listener.
    stash: HashMap<EntityId, AudioSource>,
}

impl Flow for AudioFlow {
    type View = AudioView;

    const DOMAIN: SemanticDomain = SemanticDomain::Audio;
    const NAME: &'static str = "audio";

    fn adapt(
        &mut self,
        world: &mut World,
        _sel: &Selection,
        _budget: &ResourceBudget,
        _runtime: &Runtime,
    ) {
        // AGDF: gate `AudioSource` by listener distance. Sources further
        // than [`AUDIO_DETACH_RADIUS`] from the active listener are
        // detached and stashed; they are reattached once they re-enter
        // [`AUDIO_REATTACH_RADIUS`]. Mirrors the pattern in
        // [`super::physics::PhysicsFlow`] — see that module for context.
        let Some(anchor) = active_listener_position(world) else {
            return;
        };

        // Pass 1 — detach.
        let mut to_detach: Vec<(EntityId, AudioSource)> = Vec::new();
        for (entity, transform, src) in
            world.query::<(EntityId, &GlobalTransform, &AudioSource)>()
        {
            if (transform.0.translation() - anchor).length() > AUDIO_DETACH_RADIUS {
                to_detach.push((entity, src.clone()));
            }
        }
        for (entity, snapshot) in to_detach {
            self.stash.insert(entity, snapshot);
            let _ = world.remove_component::<AudioSource>(entity);
        }

        // Pass 2 — reattach.
        let restorable: Vec<EntityId> = self
            .stash
            .keys()
            .copied()
            .filter(|e| {
                world
                    .get::<GlobalTransform>(*e)
                    .map(|t| (t.0.translation() - anchor).length() < AUDIO_REATTACH_RADIUS)
                    .unwrap_or(false)
            })
            .collect();
        for entity in restorable {
            if let Some(src) = self.stash.remove(&entity) {
                let snapshot = src.clone();
                if let Err(e) = world.add_component(entity, src) {
                    log::warn!(
                        "AudioFlow: failed to reattach AudioSource to {:?}: {:?}",
                        entity,
                        e
                    );
                    self.stash.insert(entity, snapshot);
                }
            }
        }
    }

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

fn active_listener_position(world: &World) -> Option<Vec3> {
    world
        .query::<(&AudioListener, &GlobalTransform)>()
        .next()
        .map(|(_, t)| t.0.translation())
}
