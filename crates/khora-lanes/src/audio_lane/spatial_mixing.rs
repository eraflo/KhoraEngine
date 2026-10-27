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

//! The audio family's lane: the frame's sources, spatialised and mixed.
//!
//! The lane reads the [`AudioView`](khora_data::flow::AudioView) the
//! `AudioFlow` projected, mixes one block into a buffer it keeps from frame to
//! frame, and pushes it into the shared
//! [`AudioMixBus`](khora_core::audio::AudioMixBus), which the audio backend's
//! callback drains on its real-time thread. It never touches the hardware
//! buffer and never reads the `World`.
//!
//! # Voices
//!
//! Each level mixes at most a number of voices (8, 32, 128). When more
//! sources play, the most audible ones — the highest priority first, then the
//! loudest at the listener — are mixed; the others are **virtual**: their
//! playback advances exactly as a mixed voice's would, so a virtual voice that
//! becomes audible again resumes where it would have been.
//!
//! Every source's playback state lands, in View order, in the family's
//! [`AudioPlaybackWriteback`](khora_data::flow::AudioPlaybackWriteback); the
//! `audio_playback_writeback` DataSystem patches the `AudioSource` components
//! from it in `Maintenance`.

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::audio::{AudioMixBus, StreamInfo};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::math::AffineTransform;
use khora_data::ecs::PlaybackState;
use khora_data::flow::{AudioPlaybackUpdate, AudioSourceSnapshot, AudioView};
use khora_macros::LaneInputs;

use super::audio_family::{AudioFamily, AudioOutputs};

/// Number of frames the lane mixes per record. Sized to comfortably cover one
/// display frame at 60 Hz / 48 kHz (~800 frames) with headroom.
const FRAMES_PER_TICK: usize = 1024;

/// What mixing one voice for one block costs on the host.
const MIXED_VOICE: Duration = Duration::from_micros(4);
/// What advancing one virtual voice costs on the host.
const VIRTUAL_VOICE: Duration = Duration::from_nanos(50);
/// What a block costs before any voice: clearing, limiting, pushing it.
const BLOCK: Duration = Duration::from_micros(2);

/// A level of the lane: the voices it mixes at most.
const fn voices(id: StrategyId, name: &'static str, impact: Impact) -> Level {
    Level {
        id,
        name,
        impact,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

/// The levels, cheapest first, and the voices each mixes at most.
const LEVELS: [Level; 3] = [
    voices(StrategyId::LowPower, "8 voices", Impact::Minor),
    voices(StrategyId::Balanced, "32 voices", Impact::Minor),
    voices(StrategyId::HighPerformance, "128 voices", Impact::None),
];
const VOICE_CAPS: [usize; 3] = [8, 32, 128];

/// A lane that performs spatialized audio mixing.
#[derive(Default)]
pub struct SpatialMixingLane {
    /// The block mixed this frame, kept so a steady frame allocates nothing.
    block: Vec<f32>,
    /// The sources that play this frame, by index in the View, with their
    /// rank key; kept for the same reason.
    playing: Vec<(usize, Rank)>,
    /// Whether each source of the View is mixed this frame.
    mixed: Vec<bool>,
}

/// What a lane mixes the most audible sources by: priority first, then the
/// gain at the listener.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rank {
    priority: u8,
    gain: f32,
}

/// What the mixing lane reads each frame: the sources the `AudioFlow`
/// projected, and the bus the backend's callback drains.
#[derive(LaneInputs)]
pub struct SpatialMixingInputs<'f> {
    /// The listener and every source, snapshotted for this frame.
    #[bus]
    pub view: &'f AudioView,
    /// The bus the mixed block is pushed into.
    #[resource]
    pub mix_bus: &'f Arc<dyn AudioMixBus>,
}

impl Lane for SpatialMixingLane {
    type Family = AudioFamily;
    type Inputs<'f> = SpatialMixingInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// Every playing source costs a mixed voice up to the level's cap, and a
    /// virtual one beyond it — all on the host.
    fn cost(&self, level: usize, inputs: &SpatialMixingInputs<'_>) -> Cost {
        let playing = inputs
            .view
            .sources
            .iter()
            .filter(|source| plays(source))
            .count();
        let cap = VOICE_CAPS.get(level).copied().unwrap_or(0);
        let mixed = playing.min(cap) as u32;
        let virtual_voices = playing.saturating_sub(cap) as u32;
        Cost {
            time: PerDevice {
                cpu: BLOCK + MIXED_VOICE * mixed + VIRTUAL_VOICE * virtual_voices,
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut SpatialMixingInputs<'_>,
        out: &mut AudioOutputs,
    ) -> Result<LaneReport, LaneError> {
        let cap = VOICE_CAPS
            .get(level)
            .copied()
            .ok_or(LaneError::InvalidContext {
                expected: "a voice level the lane offers",
                received: format!("level {level}"),
            })?;
        let view = inputs.view;
        let stream = inputs.mix_bus.stream_info();
        let channels = stream.channels as usize;
        self.block.clear();
        self.block.resize(FRAMES_PER_TICK * channels, 0.0);

        self.choose_voices(view, cap);
        let updates = &mut out.playback.updates;
        updates.reserve(view.sources.len());
        for (index, source) in view.sources.iter().enumerate() {
            let new_state = if self.mixed[index] {
                mix_voice(source, view.listener_transform, &stream, &mut self.block)
            } else {
                advance_voice(source, &stream)
            };
            updates.push(AudioPlaybackUpdate {
                entity: source.entity,
                new_state,
            });
        }

        // Limiter. A sample no source made finite is silence, never a NaN
        // that would silence every other source downstream.
        for sample in &mut self.block {
            *sample = if sample.is_nan() {
                0.0
            } else {
                sample.clamp(-1.0, 1.0)
            };
        }
        inputs.mix_bus.write_block(&self.block);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

impl SpatialMixingLane {
    /// Marks the `cap` most audible playing sources as mixed.
    fn choose_voices(&mut self, view: &AudioView, cap: usize) {
        self.playing.clear();
        self.playing.extend(
            view.sources
                .iter()
                .enumerate()
                .filter(|(_, source)| plays(source))
                .map(|(index, source)| (index, rank(source, view.listener_transform))),
        );
        // The highest priority, then the loudest; a tie keeps View order.
        self.playing.sort_by(|(a_index, a), (b_index, b)| {
            b.priority
                .cmp(&a.priority)
                .then(b.gain.total_cmp(&a.gain))
                .then(a_index.cmp(b_index))
        });
        self.mixed.clear();
        self.mixed.resize(view.sources.len(), false);
        for &(index, _) in self.playing.iter().take(cap) {
            self.mixed[index] = true;
        }
    }
}

/// Whether `source` plays this frame: it is started (or starts by itself),
/// has a sound to play and is not past its end, and sits at a finite place
/// with a finite volume — a source that cannot be placed is not heard.
fn plays(source: &AudioSourceSnapshot) -> bool {
    let frames = frames_of(source);
    let before_its_end = match &source.state {
        Some(state) => source.looping || state.cursor < frames as f64,
        None => source.autoplay,
    };
    let placed = [source.position.x, source.position.y, source.position.z]
        .iter()
        .all(|axis| axis.is_finite());
    before_its_end && frames > 0 && placed && source.volume.is_finite()
}

/// The number of frames in `source`'s sound; zero for an empty one.
fn frames_of(source: &AudioSourceSnapshot) -> usize {
    let sound = &source.handle;
    sound
        .samples
        .len()
        .checked_div(sound.channels as usize)
        .unwrap_or(0)
}

/// The state `source` starts the frame in: its own, or the start of its
/// sound when it plays by itself.
fn starting_state(source: &AudioSourceSnapshot) -> Option<PlaybackState> {
    match &source.state {
        Some(state) => Some(state.clone()),
        None if source.autoplay => Some(PlaybackState { cursor: 0.0 }),
        None => None,
    }
}

/// The gain and the pan `source` is heard with: the inverse-square fall-off
/// with the distance to the listener, and where it sits across the
/// listener's right. Without a listener, its own volume, centred.
fn gain_and_pan(source: &AudioSourceSnapshot, listener: Option<AffineTransform>) -> (f32, f32) {
    let Some(listener) = listener else {
        return (source.volume, 0.5);
    };
    let to_source = source.position - listener.translation();
    let distance = to_source.length();
    let gain = source.volume / (1.0 + distance * distance);
    let pan = if distance > 0.001 {
        (to_source.normalize().dot(listener.right()) + 1.0) * 0.5
    } else {
        0.5
    };
    (gain, pan)
}

/// How audible `source` is: what its voice is ranked by.
fn rank(source: &AudioSourceSnapshot, listener: Option<AffineTransform>) -> Rank {
    Rank {
        priority: source.priority,
        gain: gain_and_pan(source, listener).0,
    }
}

/// Mixes one block of `source` into `block` and returns its state after it,
/// `None` once it stopped.
fn mix_voice(
    source: &AudioSourceSnapshot,
    listener: Option<AffineTransform>,
    stream: &StreamInfo,
    block: &mut [f32],
) -> Option<PlaybackState> {
    let state = starting_state(source)?;
    let sound = &source.handle;
    let frames = frames_of(source);
    let channels = stream.channels as usize;
    if frames == 0 || channels == 0 {
        return None;
    }
    let sound_channels = sound.channels as usize;
    let ratio = playback_ratio(source, stream);
    let (gain, pan) = gain_and_pan(source, listener);
    let (left, right) = (gain * (1.0 - pan).sqrt(), gain * pan.sqrt());
    let audible = left.is_finite() && right.is_finite();
    let end = frames as f64;

    let mut cursor = state.cursor;
    for frame in 0..block.len() / channels {
        if cursor >= end {
            if !source.looping {
                return None;
            }
            cursor %= end;
        }
        let at = cursor.floor() as usize;
        let next = (at + 1) % frames;
        let (first, second) = match (
            sound.samples.get(at * sound_channels),
            sound.samples.get(next * sound_channels),
        ) {
            (Some(first), Some(second)) => (*first, *second),
            _ => return None,
        };
        if audible {
            let sample = first + (second - first) * cursor.fract() as f32;
            let out = frame * channels;
            if channels == 2 {
                block[out] += sample * left;
                block[out + 1] += sample * right;
            } else {
                block[out] += sample * gain;
            }
        }
        cursor += ratio;
    }
    Some(PlaybackState { cursor })
}

/// How many frames of `source`'s sound one frame of the stream plays.
fn playback_ratio(source: &AudioSourceSnapshot, stream: &StreamInfo) -> f64 {
    f64::from(source.handle.sample_rate) / f64::from(stream.sample_rate)
}

/// Advances a virtual voice by one block, as [`mix_voice`] would have, without
/// mixing it: the same stop, the same wraps.
fn advance_voice(source: &AudioSourceSnapshot, stream: &StreamInfo) -> Option<PlaybackState> {
    let state = starting_state(source)?;
    let frames = frames_of(source) as f64;
    if frames == 0.0 {
        return None;
    }
    let ratio = playback_ratio(source, stream);
    let block = FRAMES_PER_TICK as f64;
    let cursor = state.cursor;
    // A mixed voice checks its cursor before each frame it reads: the last
    // check of the block sees `cursor + (block - 1) × ratio`.
    let last_read = cursor + (block - 1.0) * ratio;
    let cursor = if last_read >= frames {
        if !source.looping {
            return None;
        }
        last_read.rem_euclid(frames) + ratio
    } else {
        cursor + block * ratio
    };
    Some(PlaybackState { cursor })
}
