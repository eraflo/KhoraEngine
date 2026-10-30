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

//! **A playback update lands on the source it was made for.** The mixing lane
//! writes one update per source of the `AudioView`; the
//! `audio_playback_writeback` DataSystem applies each one to that source's
//! `AudioSource`, whatever else the world holds.

use khora_core::asset::AssetHandle;
use khora_core::lane::OutputDeck;
use khora_core::math::Vec3;
use khora_core::Runtime;
use khora_data::assets::SoundData;
use khora_data::ecs::{AudioSource, DataSystemRegistration, GlobalTransform, PlaybackState, World};
use khora_data::flow::{AudioFlow, AudioPlaybackUpdate, AudioPlaybackWriteback, Flow, Selection};

/// The writeback DataSystem, as the engine collects it.
fn writeback() -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|registration| registration.name == "audio_playback_writeback")
        .expect("the audio playback writeback is registered")
}

/// A one-second tone.
fn tone() -> AssetHandle<SoundData> {
    AssetHandle::new(SoundData {
        samples: vec![0.01; 48_000],
        channels: 1,
        sample_rate: 48_000,
    })
}

/// A source with no place in the world — a level's music, say — is not in the
/// View, so it gets no update; the update of the placed source after it lands
/// on the placed source, not on the music.
#[test]
fn a_playback_update_lands_on_the_source_it_was_made_for() {
    let mut world = World::new();
    let music = world.spawn(AudioSource::new(tone()));
    let placed = world.spawn((
        AudioSource::new(tone()),
        GlobalTransform::at_position(Vec3::new(1.0, 0.0, 0.0)),
    ));
    let view = AudioFlow.project(&world, &Selection::new(), &Runtime::new());
    assert_eq!(
        view.sources.len(),
        1,
        "only the placed source is in the view"
    );

    let mut deck = OutputDeck::new();
    deck.slot::<AudioPlaybackWriteback>()
        .updates
        .push(AudioPlaybackUpdate {
            entity: view.sources[0].entity,
            new_state: Some(PlaybackState { cursor: 777.0 }),
        });
    (writeback().run)(
        &mut world,
        &Runtime::new(),
        &mut deck,
        khora_data::ecs::SystemContext {
            since: khora_data::ecs::Tick(0),
        },
    );

    let state_of = |entity| {
        world
            .get::<AudioSource>(entity)
            .and_then(|source| source.state.clone())
            .map(|state| state.cursor)
    };
    assert_eq!(
        state_of(placed),
        Some(777.0),
        "the placed source took its own update"
    );
    assert_eq!(state_of(music), None, "the music was given no update");
}
