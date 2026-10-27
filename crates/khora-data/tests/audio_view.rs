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

//! What the `AudioFlow` projects of a source: everything the mixing lane
//! ranks and mixes it by.

use khora_core::asset::AssetHandle;
use khora_core::math::Vec3;
use khora_core::Runtime;
use khora_data::assets::SoundData;
use khora_data::ecs::{AudioSource, GlobalTransform, World};
use khora_data::flow::{AudioFlow, Flow, Selection};

/// A source's priority reaches the View the mixer ranks voices from.
#[test]
fn a_source_priority_reaches_the_audio_view() {
    let mut world = World::new();
    let sound = AssetHandle::new(SoundData {
        samples: vec![0.01; 64],
        channels: 1,
        sample_rate: 48_000,
    });
    for priority in [3, 250] {
        world.spawn((
            AudioSource {
                priority,
                ..AudioSource::new(sound.clone())
            },
            GlobalTransform::at_position(Vec3::ZERO),
        ));
    }

    let view = AudioFlow.project(&world, &Selection::new(), &Runtime::new());

    let priorities: Vec<u8> = view.sources.iter().map(|s| s.priority).collect();
    assert_eq!(priorities, [3, 250]);
}
