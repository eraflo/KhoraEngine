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

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{ComponentKey, World};

/// A snapshot of entities selected by a `Flow` for the current tick.
///
/// Passed from `Flow::select` to `Flow::adapt` and `Flow::project`. The Flow
/// is free to attach its own per-domain payload by extending this struct, or
/// to keep it as just an entity list.
#[derive(Debug, Default, Clone)]
pub struct Selection {
    /// Entity IDs deemed relevant for this domain this tick.
    pub entities: Vec<EntityId>,
    /// The entities the family's scope admits, when it is scoped: those
    /// holding its marker. `None` admits every entity.
    scope: Option<HashSet<EntityId>>,
}

impl Selection {
    /// Creates an empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a selection from a pre-built entity list.
    pub fn from_entities(entities: Vec<EntityId>) -> Self {
        Self {
            entities,
            scope: None,
        }
    }

    /// This selection limited to the entities of `world` holding the marker
    /// `scope` names ([`FlowContext::scope`](super::FlowContext::scope));
    /// unlimited when `scope` is `None`.
    pub fn within(mut self, world: &World, scope: Option<ComponentKey>) -> Self {
        self.scope = scope.map(|marker| world.holders(marker).into_iter().collect());
        self
    }

    /// Whether the family's scope admits `entity`: always, unscoped.
    pub fn admits(&self, entity: EntityId) -> bool {
        self.scope
            .as_ref()
            .is_none_or(|scope| scope.contains(&entity))
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
