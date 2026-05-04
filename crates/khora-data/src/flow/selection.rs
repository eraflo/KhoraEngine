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

//! Per-frame selection produced by [`Flow::select`](super::Flow::select).

use khora_core::ecs::entity::EntityId;

/// A snapshot of entities selected by a `Flow` for the current tick.
///
/// Passed from `Flow::select` to `Flow::adapt` and `Flow::project`. The Flow
/// is free to attach its own per-domain payload by extending this struct, or
/// to keep it as just an entity list.
#[derive(Debug, Default, Clone)]
pub struct Selection {
    /// Entity IDs deemed relevant for this domain this tick.
    pub entities: Vec<EntityId>,
}

impl Selection {
    /// Creates an empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a selection from a pre-built entity list.
    pub fn from_entities(entities: Vec<EntityId>) -> Self {
        Self { entities }
    }

    /// Number of selected entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the selection is empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}
