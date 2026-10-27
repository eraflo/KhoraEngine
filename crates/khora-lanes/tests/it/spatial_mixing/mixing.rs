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

//! What one record of the mixing lane does: the levels it offers, the block it
//! pushes to the mix bus, where a source is heard, the playback it reports,
//! and what it is refused without.

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::contract::Lane;
use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{Reason, SourceKind};
use khora_core::math::Vec3;
use khora_data::ecs::PlaybackState;
use khora_lanes::audio_lane::SpatialMixingLane;

use super::fixtures::{
    constant, empty, energy, sine, sound, view_of, Mixer, Voice, ALL_VOICES, EIGHT_VOICES,
};

/// The lane offers three levels, cheapest first, under the ids GORNA knew the
/// audio agent's three tiers by.
#[test]
fn the_lane_offers_three_voice_levels_from_low_power_up() {
    let lane: SpatialMixingLane = Default::default();

    let ids: Vec<StrategyId> = lane.levels().iter().map(|level| level.id).collect();

    assert_eq!(
        ids,
        [
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance
        ]
    );
}

/// Each record pushes one block to the mix bus, whole frames of the stream's
/// channels.
#[test]
fn each_record_pushes_one_block_to_the_mix_bus() {
    let voices = [Voice::playing(&sine(4096, 48_000), 1.0)];
    let mut mixer = Mixer::stereo();

    for frame in 1..=2 {
        let mixed = mixer.mix(EIGHT_VOICES, view_of(None, &voices));
        mixed.recorded();
        assert_eq!(mixed.blocks, 1, "frame {frame}: one block");
        assert!(
            !mixed.block.is_empty(),
            "frame {frame}: the block holds samples"
        );
        assert_eq!(
            mixed.block.len() % 2,
            0,
            "frame {frame}: whole stereo frames"
        );
    }
}

/// A source to the listener's right is heard in the right channel, and not
/// in the left.
#[test]
fn a_source_to_the_right_plays_in_the_right_channel() {
    let voices = [Voice::playing(&sine(1024, 48_000), 1.0).at(Vec3::new(10.0, 0.0, 0.0))];

    let mixed = Mixer::stereo().mix(EIGHT_VOICES, view_of(Some(Vec3::ZERO), &voices));

    assert_eq!(mixed.recorded().updates.len(), 1);
    let left = energy(&mixed.block, 0, 2);
    let right = energy(&mixed.block, 1, 2);
    assert!(
        right > left * 100.0,
        "the right channel is much stronger (left {left}, right {right})"
    );
    assert!(
        left.abs() < 1e-5,
        "the left channel is silent for a source straight to the right (left {left})"
    );
}

/// Without a listener nothing is panned: both channels carry about the same.
#[test]
fn without_a_listener_both_channels_are_balanced() {
    let voices = [Voice::playing(&sine(1024, 48_000), 1.0).at(Vec3::new(10.0, 0.0, 0.0))];

    let mixed = Mixer::stereo().mix(EIGHT_VOICES, view_of(None, &voices));

    mixed.recorded();
    let left = energy(&mixed.block, 0, 2);
    let right = energy(&mixed.block, 1, 2);
    assert!(left > 0.0, "the source is heard");
    assert!(
        (left - right).abs() < left.max(right) * 0.5,
        "the channels are roughly balanced (left {left}, right {right})"
    );
}

/// A non-looping source whose sound ends in the frame is reported stopped.
#[test]
fn a_finished_source_reports_stopped() {
    let voices = [Voice::playing(&constant(0.01, 64), 1.0)];

    let mixed = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices));

    assert_eq!(mixed.recorded().updates.len(), 1);
    assert_eq!(mixed.state_of(0), None, "the source stopped");
}

/// A source whose sound holds nothing — a placeholder asset, no channel — is
/// reported stopped, and the others still play.
#[test]
fn an_empty_sound_stops_without_breaking_the_mix() {
    let voices = [
        Voice::playing(&empty(), 1.0),
        Voice::playing(&constant(0.01, 4096), 1.0),
    ];

    let mixed = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));

    mixed.recorded();
    assert_eq!(mixed.state_of(0), None, "the empty source stopped");
    assert!(mixed.cursor_of(1) > 0.0, "the other source played");
    assert!(
        mixed.block.iter().all(|sample| sample.is_finite()),
        "the mix holds no NaN"
    );
}

/// No View on the bus — no `AudioFlow` ran — refuses the lane for the frame,
/// naming the bus; nothing reaches the mix bus.
#[test]
fn the_lane_is_refused_without_an_audio_view() {
    let mixed = Mixer::stereo().mix_without_view(EIGHT_VOICES);

    match &mixed.run.outcome {
        RunOutcome::Refused(refusal) => {
            assert_eq!(refusal.source, SourceKind::Bus);
            assert_eq!(refusal.reason, Reason::Missing);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(mixed.blocks, 0, "nothing was mixed");
}

/// No mix bus in the runtime refuses the lane for the frame, naming the
/// resource.
#[test]
fn the_lane_is_refused_without_a_mix_bus() {
    let voices = [Voice::playing(&constant(0.01, 4096), 1.0)];

    let mixed = Mixer::without_mix_bus().mix(EIGHT_VOICES, view_of(None, &voices));

    match &mixed.run.outcome {
        RunOutcome::Refused(refusal) => {
            assert_eq!(refusal.source, SourceKind::Resource);
            assert_eq!(refusal.reason, Reason::Missing);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// **The block pushed to the mix bus is finite**, whatever one source holds: a
/// source at a NaN place, mixed beside an ordinary one, never turns the block
/// the device plays into NaN, silencing every other source with it.
#[test]
fn the_block_pushed_to_the_mix_bus_is_finite() {
    let tone = constant(0.01, 8192);
    let voices = [
        Voice::playing(&tone, 1.0).at(Vec3::new(1.0, 0.0, 0.0)),
        Voice::playing(&tone, 1.0).at(Vec3::new(2.0, 0.0, 0.0)),
    ];
    let mut view = view_of(Some(Vec3::ZERO), &voices);
    view.sources[1].position = Vec3::new(f32::NAN, 0.0, 0.0);

    let mixed = Mixer::stereo().mix(ALL_VOICES, view);

    mixed.recorded();
    let poisoned = mixed.block.iter().filter(|s| !s.is_finite()).count();
    assert_eq!(
        poisoned,
        0,
        "{poisoned} of {} samples pushed to the bus are not finite",
        mixed.block.len()
    );
}

/// **A voice plays at its sound's rate however far into it it is.** A
/// minute-long 44.1 kHz track mixed on a 48 kHz stream, fifty seconds in,
/// advances one block of the stream's frames resampled: 1024 × 44.1 / 48.
#[test]
fn a_voice_deep_in_a_long_track_plays_at_its_sample_rate() {
    let track = sound(vec![0.001; 44_100 * 60], 44_100);
    let start = 44_100.0 * 50.0;
    let mut voice = Voice::playing(&track, 1.0);
    voice.state = Some(PlaybackState { cursor: start });

    let mixed = Mixer::mono().mix(ALL_VOICES, view_of(None, &[voice]));

    let advanced = mixed.cursor_of(0) - start;
    let expected = 1024.0 * 44_100.0 / 48_000.0;
    assert!(
        (advanced - expected).abs() < 0.5,
        "the voice advanced {advanced} frames in one block; at its rate it is {expected}"
    );
}

/// **A voice keeps its rate deep into a long track.** A seven-minute 44.1 kHz
/// track on a 48 kHz stream, six minutes twenty in: forty-seven blocks of the
/// stream — about a second — advance it forty-seven blocks resampled, give or
/// take a frame, not a frame lost or gained each block.
#[test]
fn a_voice_keeps_its_rate_for_a_second_deep_in_a_seven_minute_track() {
    const BLOCKS: usize = 47;
    let track = sound(vec![0.001; 44_100 * 420], 44_100);
    let start = f64::from(1u32 << 24);
    let mut voice = Voice::playing(&track, 1.0);
    voice.state = Some(PlaybackState { cursor: start });
    let mut mixer = Mixer::mono();

    for _ in 0..BLOCKS {
        let mixed = mixer.mix(ALL_VOICES, view_of(None, std::slice::from_ref(&voice)));
        voice.state = mixed.state_of(0);
    }

    let advanced = voice.state.as_ref().expect("the track still plays").cursor - start;
    let expected = BLOCKS as f64 * 1024.0 * 44_100.0 / 48_000.0;
    assert!(
        (advanced - expected).abs() < 1.0,
        "the voice advanced {advanced} frames in {BLOCKS} blocks; at its rate it is {expected}"
    );
}
