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

//! Voice virtualisation, the audio family's level.
//!
//! A level caps the voices mixed in a frame. Beyond the cap the least audible
//! sources go virtual: they are not mixed, yet their playback goes on — their
//! position advances, they end or loop as they would have — so a virtual
//! voice that comes back up is heard from where it got to. The level changes
//! what is heard, never where playback is.
//!
//! Each mix is compared with a reference: the same lane, at the level that
//! caps nothing, over only the voices that should be heard. Equal blocks mean
//! exactly those voices were mixed, whatever the mixing formula.

use khora_core::math::Vec3;
use khora_data::ecs::PlaybackState;

use super::fixtures::{
    assert_same_mix, constant, max_difference, ramp, sine, sound, view_of, Mixer, Voice,
    ALL_VOICES, CAPS, EIGHT_VOICES, RATE,
};

/// The voices of `voices` at the positions `keep` lists, in order.
fn only(voices: &[Voice], keep: &[usize]) -> Vec<Voice> {
    keep.iter().map(|&k| voices[k].clone()).collect()
}

/// **Above the cap, the least audible voices go virtual.** Twenty sources play
/// at the listener's place; eight are loud and twelve quiet, interleaved. At
/// the eight-voice level the mix is the mix of the eight loud ones alone, and
/// every quiet one still advances as far as a loud one.
#[test]
fn audio_virtualises_the_least_audible_voices() {
    let tone = constant(0.01, 8192);
    let loud = [1, 4, 5, 8, 11, 13, 17, 19];
    let voices: Vec<Voice> = (0..20)
        .map(|k| Voice::playing(&tone, if loud.contains(&k) { 1.0 } else { 0.25 }))
        .collect();

    let capped = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &only(&voices, &loud)));
    let everyone = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices));

    capped.recorded();
    assert_same_mix(
        &capped.block,
        &reference.block,
        "at eight voices, the eight loudest are mixed and nothing else",
    );
    assert!(
        max_difference(&everyone.block, &reference.block) > 1e-3,
        "the quiet voices are heard when nothing caps them, so the comparison means something"
    );
    let mixed = capped.cursor_of(loud[0]);
    assert!(mixed > 0.0, "a mixed voice advanced");
    for k in 0..voices.len() {
        let cursor = capped.cursor_of(k);
        assert!(
            (cursor - mixed).abs() < 1e-3,
            "voice {k} is at {cursor}, a mixed voice at {mixed}: a virtual voice advances as if it were mixed"
        );
    }
}

/// Audibility falls with distance: at equal gain, the eight sources nearest the
/// listener are the ones mixed, wherever they sit in the view.
#[test]
fn the_nearest_voices_stay_real_at_equal_gain() {
    let tone = constant(0.01, 8192);
    let near = [0, 3, 6, 9, 12, 15, 16, 18];
    let voices: Vec<Voice> = (0..20)
        .map(|k| {
            let step = k as f32 * 0.03;
            let place = match near.iter().position(|&n| n == k) {
                Some(i) if i % 2 == 0 => Vec3::new(0.5 + step, 0.0, 0.0),
                Some(_) => Vec3::new(-(0.5 + step), 0.0, 0.3),
                None if k % 2 == 0 => Vec3::new(0.0, 0.0, 6.0 + step),
                None => Vec3::new(-(6.0 + step), 0.0, 0.0),
            };
            Voice::playing(&tone, 1.0).at(place)
        })
        .collect();
    let listener = Some(Vec3::ZERO);

    let capped = Mixer::stereo().mix(EIGHT_VOICES, view_of(listener, &voices));
    let reference = Mixer::stereo().mix(ALL_VOICES, view_of(listener, &only(&voices, &near)));
    let everyone = Mixer::stereo().mix(ALL_VOICES, view_of(listener, &voices));

    capped.recorded();
    assert_same_mix(
        &capped.block,
        &reference.block,
        "at eight voices, the eight nearest are mixed and nothing else",
    );
    assert!(
        max_difference(&everyone.block, &reference.block) > 1e-5,
        "the far voices are heard when nothing caps them"
    );
}

/// A source that is not playing is not audible: it takes no voice from one
/// that is, however loud and near it would be.
#[test]
fn a_silent_source_takes_no_voice() {
    let tone = constant(0.01, 8192);
    let mut voices = Vec::new();
    for _ in 0..8 {
        voices.push(Voice::playing(&tone, 1.0).stopped());
        voices.push(Voice::playing(&tone, 0.5));
    }
    let playing: Vec<usize> = (0..voices.len()).filter(|k| k % 2 == 1).collect();

    let capped = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &only(&voices, &playing)));

    capped.recorded();
    assert_same_mix(
        &capped.block,
        &reference.block,
        "the eight playing sources fill the eight voices",
    );
    for k in (0..voices.len()).filter(|k| k % 2 == 0) {
        assert_eq!(capped.state_of(k), None, "a stopped source stays stopped");
    }
}

/// Each level mixes as many voices as its cap, and no more: with five sources
/// over the cap, the mix is the mix of exactly `cap` of those equal sources.
#[test]
fn each_level_mixes_as_many_voices_as_its_cap() {
    let tone = constant(0.001, 4096);
    for (level, cap) in CAPS {
        let voices: Vec<Voice> = (0..cap + 5).map(|_| Voice::playing(&tone, 1.0)).collect();

        let capped = Mixer::mono().mix(level, view_of(None, &voices));
        let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices[..cap]));

        let playback = capped.recorded();
        assert!(
            capped.block.iter().any(|sample| *sample > 0.0),
            "level {level} mixes something"
        );
        assert_same_mix(
            &capped.block,
            &reference.block,
            &format!("level {level} mixes {cap} voices"),
        );
        assert_eq!(
            playback.updates.len(),
            cap + 5,
            "level {level}: every source, mixed or not, gets its playback update"
        );
        for k in 0..voices.len() {
            assert!(
                capped.cursor_of(k) > 0.0,
                "level {level}: voice {k} advanced"
            );
        }
    }
}

/// **A virtual voice turns real when it becomes audible.** Nine sources, eight
/// voices: the quiet ninth is virtual on the first frame and still advances.
/// On the second frame it is loud and another has faded: the late one is
/// mixed from where it got to while virtual, and the faded one goes virtual.
#[test]
fn a_virtual_voice_turns_real_when_it_becomes_audible() {
    const LATE: usize = 8;
    const FADED: usize = 3;
    let track = ramp(8192);
    let mut voices: Vec<Voice> = (0..9)
        .map(|k| Voice::playing(&track, if k == LATE { 0.1 } else { 1.0 }))
        .collect();
    let mut mixer = Mixer::mono();

    let first = mixer.mix(EIGHT_VOICES, view_of(None, &voices));
    let late = first.cursor_of(LATE);
    let real = first.cursor_of(0);
    assert!(late > 0.0, "the virtual voice advanced on the first frame");
    assert!(
        (late - real).abs() < 1e-3,
        "the virtual voice is at {late}, a mixed one at {real}"
    );

    // The frame's writeback, applied as the DataSystem applies it.
    for (k, voice) in voices.iter_mut().enumerate() {
        voice.state = first.state_of(k);
    }
    voices[LATE].volume = 1.0;
    voices[FADED].volume = 0.1;
    let second = mixer.mix(EIGHT_VOICES, view_of(None, &voices));

    let audible: Vec<usize> = (0..voices.len()).filter(|&k| k != FADED).collect();
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &only(&voices, &audible)));
    assert_same_mix(
        &second.block,
        &reference.block,
        "the late voice is mixed from where it got to, and the faded one is not mixed",
    );

    let mut restarted = only(&voices, &audible);
    let late_in_audible = audible.len() - 1;
    restarted[late_in_audible].state = None;
    let from_the_start = Mixer::mono().mix(ALL_VOICES, view_of(None, &restarted));
    assert!(
        max_difference(&from_the_start.block, &reference.block) > 1e-5,
        "where the late voice is in its sound can be heard, so the comparison means something"
    );

    let faded = second.cursor_of(FADED);
    let late = second.cursor_of(LATE);
    assert!(
        (faded - late).abs() < 1e-3,
        "the faded voice, virtual now, is at {faded}; the late one, mixed, at {late}"
    );
}

/// A virtual voice advances as far as it would have mixed, resampling
/// included: the sound plays at 44.1 kHz on a 48 kHz stream.
#[test]
fn a_virtual_voice_advances_as_far_as_a_real_one() {
    let loud_tone = constant(0.01, 8192);
    let resampled = sine(8192, 44_100);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&loud_tone, 1.0)).collect();
    voices.push(Voice::playing(&resampled, 0.05));
    let target = voices.len() - 1;
    let mut capped = Mixer::stereo();
    let mut alone = Mixer::stereo();
    let mut alone_voices = vec![voices[target].clone()];

    for frame in 1..=3 {
        let virtual_run = capped.mix(EIGHT_VOICES, view_of(None, &voices));
        let real_run = alone.mix(ALL_VOICES, view_of(None, &alone_voices));
        let virtual_cursor = virtual_run.cursor_of(target);
        let real_cursor = real_run.cursor_of(0);
        assert!(
            (virtual_cursor - real_cursor).abs() < 0.5,
            "frame {frame}: virtual at {virtual_cursor}, mixed at {real_cursor}"
        );
        for (k, voice) in voices.iter_mut().enumerate() {
            voice.state = virtual_run.state_of(k);
        }
        alone_voices[0].state = real_run.state_of(0);
    }
}

/// A virtual voice whose sound ends during the frame stops, as a mixed one
/// does: its update says stopped.
#[test]
fn a_virtual_voice_stops_at_the_end_of_its_sound() {
    let loud_tone = constant(0.01, 8192);
    let blip = constant(0.01, 64);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&loud_tone, 1.0)).collect();
    voices.push(Voice::playing(&blip, 0.05));
    let target = voices.len() - 1;

    let capped = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));
    let alone = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices[target..]));

    assert_eq!(
        alone.state_of(0),
        None,
        "mixed, the short sound ends in the frame"
    );
    assert_eq!(
        capped.state_of(target),
        None,
        "virtual, the short sound ends in the frame too"
    );
}

/// A looping virtual voice wraps around its sound as a mixed one does.
#[test]
fn a_looping_virtual_voice_wraps_like_a_real_one() {
    const FRAMES: usize = 100;
    let loud_tone = constant(0.01, 8192);
    let short_loop = sound((0..FRAMES).map(|i| i as f32 * 1e-4).collect(), RATE);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&loud_tone, 1.0)).collect();
    voices.push(Voice::playing(&short_loop, 0.05).looping());
    let target = voices.len() - 1;

    let capped = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));
    let alone = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices[target..]));

    let virtual_cursor = capped.cursor_of(target);
    let real_cursor = alone.cursor_of(0);
    let apart = (virtual_cursor - real_cursor).rem_euclid(FRAMES as f64);
    assert!(
        apart < 0.5 || apart > FRAMES as f64 - 0.5,
        "virtual at {virtual_cursor}, mixed at {real_cursor}, around a loop of {FRAMES} frames"
    );
}

/// The writeback DataSystem applies updates by position, so the lane lists
/// one update per source in the view's order, mixed or virtual alike.
#[test]
fn every_source_gets_its_update_in_view_order() {
    let tone = constant(0.01, 8192);
    let voices: Vec<Voice> = (0..20)
        .map(|k| Voice::playing(&tone, if k % 3 == 0 { 1.0 } else { 0.2 }))
        .collect();

    let capped = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));

    let updated: Vec<_> = capped.recorded().updates.iter().map(|u| u.entity).collect();
    let listed: Vec<_> = capped.view.sources.iter().map(|s| s.entity).collect();
    assert_eq!(
        updated, listed,
        "one update per source, in the view's order"
    );
}

/// **A virtual voice keeps pace with a mixed one however far into its sound
/// it is.** A minute-long track at 44.1 kHz, fifty seconds in, goes virtual for
/// a second beside eight louder voices; the same track mixed alone over that
/// second is where the virtual one is, frame after frame.
#[test]
fn a_virtual_voice_keeps_pace_with_a_mixed_one_deep_in_a_long_track() {
    let track = sound(vec![0.001; 44_100 * 60], 44_100);
    let loud_tone = constant(0.01, 1 << 16);
    let mut voices: Vec<Voice> = (0..8)
        .map(|_| Voice::playing(&loud_tone, 1.0).looping())
        .collect();
    let mut deep = Voice::playing(&track, 0.05);
    deep.state = Some(PlaybackState {
        cursor: 44_100.0 * 50.0,
    });
    voices.push(deep.clone());
    let target = voices.len() - 1;
    let mut alone_voices = vec![deep];
    let mut capped = Mixer::mono();
    let mut alone = Mixer::mono();

    for frame in 1..=47 {
        let virtual_run = capped.mix(EIGHT_VOICES, view_of(None, &voices));
        let real_run = alone.mix(ALL_VOICES, view_of(None, &alone_voices));
        let virtual_cursor = virtual_run.cursor_of(target);
        let real_cursor = real_run.cursor_of(0);
        assert!(
            (virtual_cursor - real_cursor).abs() < 0.5,
            "frame {frame}: virtual at {virtual_cursor}, mixed at {real_cursor}"
        );
        for (k, voice) in voices.iter_mut().enumerate() {
            voice.state = virtual_run.state_of(k);
        }
        alone_voices[0].state = real_run.state_of(0);
    }
}

/// **A voice whose sound has ended takes no voice.** Eight one-shots exactly
/// one block long are mixed on the first frame and end there; on the second
/// frame the quiet ninth source, virtual on the first, is the one mixed.
#[test]
fn a_voice_whose_sound_ended_takes_no_voice_from_one_still_playing() {
    const LATE: usize = 8;
    let blip = constant(0.01, 1024);
    let track = ramp(8192);
    let mut voices: Vec<Voice> = (0..8).map(|_| Voice::playing(&blip, 1.0)).collect();
    voices.push(Voice::playing(&track, 0.5));
    let mut mixer = Mixer::mono();

    let first = mixer.mix(EIGHT_VOICES, view_of(None, &voices));
    // The frame's writeback, applied as the DataSystem applies it.
    for (k, voice) in voices.iter_mut().enumerate() {
        voice.state = first.state_of(k);
    }
    let second = mixer.mix(EIGHT_VOICES, view_of(None, &voices));
    let reference = Mixer::mono().mix(ALL_VOICES, view_of(None, &voices[LATE..]));

    assert_same_mix(
        &second.block,
        &reference.block,
        "the ended one-shots give their voices to the source still playing",
    );
}

/// **A source at a place that is not a number is not audible**: it takes no
/// voice from the sources that are. Nine sources play at eight voices, one of
/// them at a NaN place; the mix is the mix of the eight others.
#[test]
fn a_source_at_a_non_finite_place_takes_no_voice() {
    let tone = constant(0.01, 8192);
    let voices: Vec<Voice> = (0..9)
        .map(|k| Voice::playing(&tone, 1.0).at(Vec3::new(1.0 + k as f32 * 0.1, 0.0, 0.0)))
        .collect();
    let mut view = view_of(Some(Vec3::ZERO), &voices);
    view.sources[8].position = Vec3::new(f32::NAN, 0.0, 0.0);

    let capped = Mixer::stereo().mix(EIGHT_VOICES, view);
    let reference = Mixer::stereo().mix(ALL_VOICES, view_of(Some(Vec3::ZERO), &voices[..8]));

    capped.recorded();
    assert!(
        capped.block.iter().all(|sample| sample.is_finite()),
        "the source at a NaN place was mixed: the block holds NaN"
    );
    assert_same_mix(
        &capped.block,
        &reference.block,
        "the eight sources at a place are the eight mixed",
    );
}
