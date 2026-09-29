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

//! An agent's set of lanes, looked up by name and kind.

use super::kind::LaneKind;
use super::Lane;

// ─────────────────────────────────────────────────────────────────────────────
// LaneRegistry — generic container for heterogeneous lanes
// ─────────────────────────────────────────────────────────────────────────────

/// A registry that stores [`Lane`] trait objects for agent use.
///
/// Agents use a `LaneRegistry` instead of domain-specific vectors
/// (e.g., `Vec<Box<dyn RenderLane>>`). This enables developers to add
/// custom lanes without modifying agent code.
///
/// ```rust,ignore
/// use khora_core::lane::{LaneRegistry, LaneKind};
///
/// let mut reg = LaneRegistry::new();
/// reg.register(Box::new(MyCustomLane::new()));
///
/// // Find all render lanes
/// let render_lanes = reg.find_by_kind(LaneKind::Render);
/// ```
pub struct LaneRegistry {
    lanes: Vec<Box<dyn Lane>>,
}

impl LaneRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self { lanes: Vec::new() }
    }

    /// Adds a lane to the registry.
    pub fn register(&mut self, lane: Box<dyn Lane>) {
        self.lanes.push(lane);
    }

    /// Finds a lane by its strategy name.
    pub fn get(&self, name: &str) -> Option<&dyn Lane> {
        self.lanes
            .iter()
            .find(|l| l.strategy_name() == name)
            .map(|b| b.as_ref())
    }

    /// Returns all lanes of a given kind.
    pub fn find_by_kind(&self, kind: LaneKind) -> Vec<&dyn Lane> {
        self.lanes
            .iter()
            .filter(|l| l.lane_kind() == kind)
            .map(|b| b.as_ref())
            .collect()
    }

    /// Returns a slice of all registered lanes.
    pub fn all(&self) -> &[Box<dyn Lane>] {
        &self.lanes
    }

    /// Returns the number of registered lanes.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    /// Returns `true` if no lanes are registered.
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }
}

impl Default for LaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}
