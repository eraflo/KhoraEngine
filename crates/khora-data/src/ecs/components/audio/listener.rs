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

//! Defines the `AudioListener` component for 3D spatial audio.

use khora_macros::Component;

/// An ECS component that defines the point of audition in the scene.
///
/// There should typically be only one `AudioListener` in a `World`, attached
/// to the entity that represents the player or the main camera. Its `GlobalTransform`
/// will be used by the audio system to calculate 3D audio spatialization effects
/// like panning and attenuation.
#[derive(Debug, Default, Clone, Copy, Component)]
pub struct AudioListener;
