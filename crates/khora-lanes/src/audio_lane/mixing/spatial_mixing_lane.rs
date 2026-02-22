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

//! The core audio processing lane, responsible for mixing and spatializing sound sources.

use khora_core::audio::device::StreamInfo;
use khora_data::ecs::{AudioListener, AudioSource, GlobalTransform, PlaybackState, World};

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

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{AudioOutputSlot, AudioStreamInfo, LaneError, Slot};

        let stream_info = ctx
            .get::<AudioStreamInfo>()
            .ok_or(LaneError::missing("AudioStreamInfo"))?
            .0;
        let output_slot = ctx
            .get::<AudioOutputSlot>()
            .ok_or(LaneError::missing("AudioOutputSlot"))?;
        let output_buffer = output_slot.get();
        let world = ctx
            .get::<Slot<World>>()
            .ok_or(LaneError::missing("Slot<World>"))?
            .get();

        self.mix(world, output_buffer, &stream_info);
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
    /// Mixes all active `AudioSource`s into a single output buffer, applying 3D spatialization.
    pub fn mix(&self, world: &mut World, output_buffer: &mut [f32], stream_info: &StreamInfo) {
        output_buffer.fill(0.0);

        // --- Step 1: Find the listener (if any) ---
        let listener_transform = world
            .query::<(&AudioListener, &GlobalTransform)>()
            .next()
            .map(|(_, t)| t.0);

        // --- Step 2 & 3: Process and mix all active sources ---
        let samples_to_write = output_buffer.len() / stream_info.channels as usize;

        for (source, source_transform) in world.query_mut::<(&mut AudioSource, &GlobalTransform)>()
        {
            if source.autoplay && source.state.is_none() {
                source.state = Some(PlaybackState { cursor: 0.0 });
            }

            let sound_data = &source.handle;
            let num_frames = sound_data.samples.len() / sound_data.channels as usize;

            // Stop immediately if the sound is empty.
            if num_frames == 0 {
                source.state = None;
                continue;
            }

            let resample_ratio = sound_data.sample_rate as f32 / stream_info.sample_rate as f32;
            let (mut volume, mut pan) = (source.volume, 0.5);

            if let Some(listener_mat) = listener_transform {
                let source_pos = source_transform.0.translation();
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

            for i in 0..samples_to_write {
                // Get a mutable reference to the cursor for this iteration.
                // If the state becomes None mid-loop, we stop processing this source.
                let cursor = if let Some(state) = source.state.as_mut() {
                    &mut state.cursor
                } else {
                    break;
                };

                // --- Robust End-of-Sound and Loop Handling ---
                if *cursor >= num_frames as f32 {
                    if source.looping {
                        *cursor %= num_frames as f32;
                    } else {
                        source.state = None;
                        break; // Stop processing samples for this source
                    }
                }

                let cursor_floor = cursor.floor() as usize;
                let cursor_fract = cursor.fract();

                // For looping sounds, the next sample might wrap around to the beginning.
                let next_frame_idx = (cursor_floor + 1) % num_frames;

                let s1_idx = cursor_floor * sound_data.channels as usize;
                let s2_idx = next_frame_idx * sound_data.channels as usize;

                // This check prevents panics if sound data is malformed, though unlikely.
                if s1_idx >= sound_data.samples.len() || s2_idx >= sound_data.samples.len() {
                    source.state = None;
                    break;
                }

                let s1 = sound_data.samples[s1_idx];
                let s2 = sound_data.samples[s2_idx];
                let sample = s1 + (s2 - s1) * cursor_fract;

                // Mix into output buffer
                let out_idx = i * stream_info.channels as usize;
                if stream_info.channels == 2 {
                    output_buffer[out_idx] += sample * vol_l;
                    output_buffer[out_idx + 1] += sample * vol_r;
                } else {
                    output_buffer[out_idx] += sample * volume;
                }

                // Advance cursor
                *cursor += resample_ratio;
            }
        }

        // --- Step 4: Limiter ---
        for sample in output_buffer.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::{
        asset::AssetHandle,
        math::{affine_transform::AffineTransform, vector::Vec3},
    };
    use khora_data::assets::SoundData;

    // Helper to create a simple SoundData for tests.
    fn create_test_sound(len: usize, sample_rate: u32) -> AssetHandle<SoundData> {
        let samples = (0..len).map(|i| (i as f32).sin()).collect();
        AssetHandle::new(SoundData {
            samples,
            channels: 1, // Mono for simplicity
            sample_rate,
        })
    }

    // Helper for floating-point comparison
    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_panning_right() {
        let mut world = World::new();
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 44100,
        };
        let lane = SpatialMixingLane::new();
        let mut buffer = vec![0.0; 128];

        // Listener at the origin
        world.spawn((AudioListener, GlobalTransform(AffineTransform::IDENTITY)));

        // Source sound to the right of the listener
        let sound = create_test_sound(1024, 44100);
        world.spawn((
            AudioSource {
                handle: sound,
                autoplay: true,
                looping: false,
                volume: 1.0,
                state: None,
            },
            GlobalTransform(AffineTransform::from_translation(Vec3::new(10.0, 0.0, 0.0))),
        ));

        lane.mix(&mut world, &mut buffer, &stream_info);

        // The sum of squares is a good measure of the signal's energy
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

        // With a sound perfectly to the right, the left channel MUST be silent.
        assert!(
            approx_eq(energy_left, 0.0),
            "The left channel should be silent for a sound perfectly to the right"
        );
    }

    #[test]
    fn test_distance_attenuation() {
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 44100,
        };
        let lane = SpatialMixingLane::new();

        // --- Case 1: Near source ---
        let mut world_near = World::new();
        world_near.spawn((AudioListener, GlobalTransform(AffineTransform::IDENTITY)));
        let sound = create_test_sound(1024, 44100);
        world_near.spawn((
            AudioSource {
                handle: sound.clone(),
                autoplay: true,
                looping: true,
                volume: 1.0,
                state: None,
            },
            GlobalTransform(AffineTransform::from_translation(Vec3::new(1.0, 0.0, 0.0))),
        ));

        let mut buffer_near = vec![0.0; 128];
        lane.mix(&mut world_near, &mut buffer_near, &stream_info);
        let peak_near = buffer_near.iter().map(|s| s.abs()).fold(0.0, f32::max);

        // --- Case 2: Far source ---
        let mut world_far = World::new();
        world_far.spawn((AudioListener, GlobalTransform(AffineTransform::IDENTITY)));
        world_far.spawn((
            AudioSource {
                handle: sound,
                autoplay: true,
                looping: true,
                volume: 1.0,
                state: None,
            },
            GlobalTransform(AffineTransform::from_translation(Vec3::new(
                100.0, 0.0, 0.0,
            ))),
        ));

        let mut buffer_far = vec![0.0; 128];
        lane.mix(&mut world_far, &mut buffer_far, &stream_info);
        let peak_far = buffer_far.iter().map(|s| s.abs()).fold(0.0, f32::max);

        assert!(peak_near > 0.01, "The near sound should be audible");
        assert!(
            peak_far < peak_near * 0.1,
            "The far sound should be significantly quieter"
        );
    }

    #[test]
    fn test_sound_finishes_and_stops() {
        let mut world = World::new();
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 10,
        };
        let lane = SpatialMixingLane::new();

        let sound = create_test_sound(5, 10); // Sound of 5 samples at 10Hz (0.5s)
        let entity = world.spawn((
            AudioSource {
                handle: sound,
                autoplay: true,
                looping: false, // Does not loop
                volume: 1.0,
                state: None,
            },
            GlobalTransform::default(),
        ));

        // First mix, the sound plays
        let mut buffer = vec![0.0; 20]; // 20 samples = 2 seconds
        lane.mix(&mut world, &mut buffer, &stream_info);

        // After the mix, the sound should have finished
        let source = world.get_mut::<AudioSource>(entity).unwrap();
        assert!(
            source.state.is_none(),
            "The playback state should be `None` after the sound finishes"
        );

        // Check that the sound stops in the buffer
        let first_part_energy = buffer[0..10].iter().map(|&s| s * s).sum::<f32>();
        let second_part_energy = buffer[10..20].iter().map(|&s| s * s).sum::<f32>();
        assert!(first_part_energy > 0.0);
        assert!(
            approx_eq(second_part_energy, 0.0),
            "The second half of the buffer should be silent"
        );
    }

    #[test]
    fn test_sound_loops() {
        let mut world = World::new();
        let stream_info = StreamInfo {
            channels: 1,
            sample_rate: 10,
        };
        let lane = SpatialMixingLane::new();

        let sound = create_test_sound(5, 10); // Sound of 5 samples
        let entity = world.spawn((
            AudioSource {
                handle: sound,
                autoplay: true,
                looping: true, // Loops
                volume: 1.0,
                state: None,
            },
            GlobalTransform::default(),
        ));

        let mut buffer = vec![0.0; 12]; // Buffer of 12 samples
        lane.mix(&mut world, &mut buffer, &stream_info);

        // After the mix, the sound should still be playing
        let source = world.get::<AudioSource>(entity).unwrap();
        let cursor = source.state.as_ref().unwrap().cursor;
        assert!(
            cursor > 0.0 && cursor < 5.0,
            "The cursor should have looped and returned to the beginning, cursor is {}",
            cursor
        );

        // The sound should be present at the start and end of the buffer
        assert!(!approx_eq(buffer[1], 0.0)); // Beginning
        assert!(!approx_eq(buffer[6], 0.0)); // Middle (after loop)
        assert!(!approx_eq(buffer[11], 0.0)); // End
    }
}
