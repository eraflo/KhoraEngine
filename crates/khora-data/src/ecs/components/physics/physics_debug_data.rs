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

use khora_core::math::Vec3;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The physics provider's debug wireframe, drawn when enabled; runtime state, never saved.
#[derive(Debug, Clone, Default, Component, Serialize, Deserialize)]
#[component(domain = Physics, provenance = Runtime)]
pub struct PhysicsDebugData {
    /// The wireframe's vertices, in world space.
    pub vertices: Vec<Vec3>,
    /// The wireframe's lines, as pairs of indices into `vertices`.
    pub indices: Vec<[u32; 2]>,
    /// Whether the wireframe is drawn.
    pub enabled: bool,
}
