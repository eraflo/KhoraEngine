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

//! Shadow data routed lane-to-lane through the per-frame
//! [`OutputDeck`](khora_core::lane::OutputDeck).
//!
//! `shadow_pass_lane` (in OBSERVE) writes a [`ShadowEntries`] map keyed by
//! the light's index in `RenderWorld.lights`. Lit render lanes (`lit_forward`,
//! `forward_plus`) in OUTPUT read the same map and look up shadow data per
//! light. This replaces the previous in-place mutation of
//! `RenderWorld.lights[i].shadow_view_proj/atlas_index`, which was no longer
//! possible once `RenderWorld` became a read-only View in the LaneBus.

use std::collections::HashMap;

use khora_core::math::Mat4;

/// Shadow data computed by the shadow pass for a single light.
#[derive(Debug, Clone, Copy)]
pub struct ShadowEntry {
    /// Light's view-projection matrix used to sample the shadow atlas.
    pub view_proj: Mat4,
    /// Layer index inside the shadow atlas.
    pub atlas_index: i32,
}

/// Per-frame shadow lookup keyed by light index in `RenderWorld.lights`.
#[derive(Debug, Default, Clone)]
pub struct ShadowEntries(pub HashMap<usize, ShadowEntry>);

impl ShadowEntries {
    /// Inserts (or replaces) shadow data for the light at `light_index`.
    pub fn insert(&mut self, light_index: usize, entry: ShadowEntry) {
        self.0.insert(light_index, entry);
    }

    /// Looks up shadow data for the light at `light_index`.
    pub fn get(&self, light_index: usize) -> Option<&ShadowEntry> {
        self.0.get(&light_index)
    }

    /// Number of shadow entries currently recorded.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no shadow entries are currently recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
