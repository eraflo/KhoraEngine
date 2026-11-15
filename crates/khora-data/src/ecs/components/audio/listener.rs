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

//! [`AudioListener`], the point a scene is heard from.

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The point a scene is heard from, placed by its entity's `GlobalTransform`.
///
/// Keep one per world, usually on the player or the main camera; panning and
/// attenuation are computed from its pose.
// `Authored`, hence saved: a scene has an ear because somebody placed one.
#[derive(Debug, Default, Clone, Copy, Component, Serialize, Deserialize)]
#[component(domain = Audio)]
pub struct AudioListener;
