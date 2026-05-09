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

//! The core audio processing lane, responsible for mixing and
//! spatializing sound sources.
//!
//! Per CLAD doctrine the lane consumes a typed
//! [`AudioView`](khora_data::flow::AudioView) from the
//! [`LaneBus`](khora_core::lane::LaneBus), runs the spatialised mix into
//! a per-frame staging buffer, and pushes the result into a shared
//! [`AudioMixBus`](khora_core::audio::AudioMixBus). The audio backend's
//! callback drains the bus on its dedicated real-time thread. The lane
//! never touches the hardware buffer directly and never queries
//! `World`.
//!
//! Per-source playback updates land in an
//! [`AudioPlaybackWriteback`](khora_data::flow::AudioPlaybackWriteback)
//! slot of the per-frame `OutputDeck`; the
//! `audio_playback_writeback` `DataSystem` drains that slot in
//! `Maintenance` and patches `AudioSource` components.

use std::sync::Arc;

use khora_core::audio::{AudioMixBus, StreamInfo};
use khora_core::lane::{LaneError, OutputDeck, Ref, Slot};
use khora_data::ecs::PlaybackState;
use khora_data::flow::{AudioPlaybackUpdate, AudioPlaybackWriteback, AudioView};

/// Number of frames the lane mixes per `execute` call. Sized to comfortably
/// cover one display frame at 60 Hz / 48 kHz (~800 frames) with headroom.
const FRAMES_PER_TICK: usize = 1024;

/// A lane that performs spatialized audio mixing.
#[derive(Default)]
pub struct SpatialMixingLane;

impl SpatialMixingLane {
    /// Creates a new `SpatialMixingLane`.
    pub fn new() -> Self {
        Self
    }
}

impl khora_core::lane::Lane for SpatialMixingLane {
    fn strategy_name(&self) -> &'static str {
        "SpatialMixing"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Audio
    }

    fn execute(&self, ctx: &mut khora_core::lane::LaneContext) -> Result<(), LaneError> {
        let mix_bus = ctx
            .get::<Arc<dyn AudioMixBus>>()
            .ok_or(LaneError::missing("Arc<dyn AudioMixBus>"))?
            .clone();
        let view = ctx
            .get::<Ref<AudioView>>()
            .ok_or(LaneError::missing("Ref<AudioView>"))?
            .get();

        let stream_info = mix_bus.stream_info();
        let sample_count = FRAMES_PER_TICK * stream_info.channels as usize;
        let mut staging = vec![0.0_f32; sample_count];

        let writeback = self.mix(view, &mut staging, &stream_info);
        mix_bus.write_block(&staging);

        if let Some(deck_slot) = ctx.get::<Slot<OutputDeck>>() {
            let deck = deck_slot.get();
            let slot = deck.slot::<AudioPlaybackWriteback>();
            slot.updates.extend(writeback.updates);
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl SpatialMixingLane {
    /// Mixes the snapshotted `AudioView` into `output_buffer` and
    /// returns one [`AudioPlaybackUpdate`] per source whose state
    /// changed. The updates are **not** applied to ECS components here
    /// — that is the `audio_playback_writeback` DataSystem's job.
    pub fn mix(
        &self,
        view: &AudioView,
        output_buffer: &mut [f32],
        stream_info: &StreamInfo,
    ) -> AudioPlaybackWriteback {
        output_buffer.fill(0.0);

        let mut writeback = AudioPlaybackWriteback {
            updates: Vec::with_capacity(view.sources.len()),
        };

        let listener_transform = view.listener_transform;
        let samples_to_write = output_buffer.len() / stream_info.channels as usize;

        for source in &view.sources {
            // Local copy of the playback state — mutated below, then
            // emitted in the writeback. The component itself is never
            // touched here.
            let mut state = source.state.clone();

            if source.autoplay && state.is_none() {
                state = Some(PlaybackState { cursor: 0.0 });
            }

            let sound_data = &source.handle;

            // A freshly added `AudioSource` (or a placeholder asset) may
            // have `channels == 0` and/or no samples. Treat both as
            // "empty source — emit a stop and skip" so the lane never
            // divides by zero.
            let channels = sound_data.channels as usize;
            let num_frames = if channels == 0 {
                0
            } else {
                sound_data.samples.len() / channels
            };
            if num_frames == 0 {
                writeback.updates.push(AudioPlaybackUpdate {
                    entity: source.entity,
                    new_state: None,
                });
                continue;
            }

            let resample_ratio = sound_data.sample_rate as f32 / stream_info.sample_rate as f32;
            let (mut volume, mut pan) = (source.volume, 0.5);

            if let Some(listener_mat) = listener_transform {
                let source_pos = source.position;
                let listener_pos = listener_mat.translation();
                let listener_right = listener_mat.right();
                let to_source = source_pos - listener_pos;
                let distance = to_source.length();

                volume *= 1.0 / (1.0 + distance * distance);
                if distance > 0.001 {
                    pan = (to_source.normalize().dot(listener_right) + 1.0) * 0.5;
                }
            }

            let vol_l = volume * (1.0 - pan).sqrt();
            let vol_r = volume * pan.sqrt();

            let mut stopped = false;
            for i in 0..samples_to_write {
                let cursor = if let Some(s) = state.as_mut() {
                    &mut s.cursor
                } else {
                    stopped = true;
                    break;
                };

                if *cursor >= num_frames as f32 {
                    if source.looping {
                        *cursor %= num_frames as f32;
                    } else {
                        stopped = true;
                        break;
                    }
                }

                let cursor_floor = cursor.floor() as usize;
                let cursor_fract = cursor.fract();

                let next_frame_idx = (cursor_floor + 1) % num_frames;

                let s1_idx = cursor_floor * sound_data.channels as usize;
                let s2_idx = next_frame_idx * sound_data.channels as usize;

                if s1_idx >= sound_data.samples.len() || s2_idx >= sound_data.samples.len() {
                    stopped = true;
                    break;
                }

                let s1 = sound_data.samples[s1_idx];
                let s2 = sound_data.samples[s2_idx];
                let sample = s1 + (s2 - s1) * cursor_fract;

                let out_idx = i * stream_info.channels as usize;
                if stream_info.channels == 2 {
                    output_buffer[out_idx] += sample * vol_l;
                    output_buffer[out_idx + 1] += sample * vol_r;
                } else {
                    output_buffer[out_idx] += sample * volume;
                }

                *cursor += resample_ratio;
            }

            writeback.updates.push(AudioPlaybackUpdate {
                entity: source.entity,
                new_state: if stopped { None } else { state },
            });
        }

        // Limiter.
        for sample in output_buffer.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        writeback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;
    use khora_core::{
        asset::AssetHandle,
        math::{affine_transform::AffineTransform, vector::Vec3},
    };
    use khora_data::assets::SoundData;
    use khora_data::flow::AudioSourceSnapshot;

    fn create_test_sound(len: usize, sample_rate: u32) -> AssetHandle<SoundData> {
        let samples = (0..len).map(|i| (i as f32).sin()).collect();
        AssetHandle::new(SoundData {
            samples,
            channels: 1,
            sample_rate,
        })
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn make_view_panning_right() -> AudioView {
        AudioView {
            source_count: 1,
            listener_position: Some(Vec3::ZERO),
            listener_transform: Some(AffineTransform::IDENTITY),
            sources: vec![AudioSourceSnapshot {
                entity: EntityId { index: 0, generation: 0 },
                handle: create_test_sound(1024, 44100),
                position: Vec3::new(10.0, 0.0, 0.0),
                volume: 1.0,
                looping: false,
                autoplay: true,
                state: None,
            }],
        }
    }

    #[test]
    fn test_panning_right() {
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 44100,
        };
        let lane = SpatialMixingLane::new();
        let mut buffer = vec![0.0; 128];
        let view = make_view_panning_right();

        let writeback = lane.mix(&view, &mut buffer, &stream_info);
        assert_eq!(writeback.updates.len(), 1);

        let energy_left = buffer.iter().step_by(2).map(|&s| s * s).sum::<f32>();
        let energy_right = buffer
            .iter()
            .skip(1)
            .step_by(2)
            .map(|&s| s * s)
            .sum::<f32>();

        assert!(
            energy_right > energy_left * 100.0,
            "The energy should be much stronger in the right channel"
        );
        assert!(
            approx_eq(energy_left, 0.0),
            "The left channel should be silent for a sound perfectly to the right"
        );
    }

    #[test]
    fn test_no_listener_no_panning() {
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 44100,
        };
        let lane = SpatialMixingLane::new();
        let mut buffer = vec![0.0; 128];
        let view = AudioView {
            source_count: 1,
            listener_position: None,
            listener_transform: None,
            sources: vec![AudioSourceSnapshot {
                entity: EntityId { index: 0, generation: 0 },
                handle: create_test_sound(1024, 44100),
                position: Vec3::new(10.0, 0.0, 0.0),
                volume: 1.0,
                looping: false,
                autoplay: true,
                state: None,
            }],
        };

        let _ = lane.mix(&view, &mut buffer, &stream_info);
        let energy_left = buffer.iter().step_by(2).map(|&s| s * s).sum::<f32>();
        let energy_right = buffer
            .iter()
            .skip(1)
            .step_by(2)
            .map(|&s| s * s)
            .sum::<f32>();
        assert!(
            (energy_left - energy_right).abs() < energy_left.max(energy_right) * 0.5,
            "Without a listener the channels should be roughly balanced"
        );
    }

    #[test]
    fn writeback_marks_finished_source_as_stopped() {
        let stream_info = StreamInfo {
            channels: 1,
            sample_rate: 44100,
        };
        let lane = SpatialMixingLane::new();
        // Buffer larger than the sound — ensures the cursor reaches end.
        let mut buffer = vec![0.0; 4096];
        let view = AudioView {
            source_count: 1,
            listener_position: None,
            listener_transform: None,
            sources: vec![AudioSourceSnapshot {
                entity: EntityId { index: 0, generation: 0 },
                handle: create_test_sound(64, 44100),
                position: Vec3::ZERO,
                volume: 1.0,
                looping: false,
                autoplay: true,
                state: None,
            }],
        };

        let wb = lane.mix(&view, &mut buffer, &stream_info);
        assert_eq!(wb.updates.len(), 1);
        assert!(
            wb.updates[0].new_state.is_none(),
            "Non-looping source should report stopped state"
        );
    }
}
