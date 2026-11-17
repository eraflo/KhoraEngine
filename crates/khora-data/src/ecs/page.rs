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

use khora_core::ecs::entity::EntityId;

use crate::ecs::{ColumnMap, ColumnTicks, ComponentKey, Tick};

pub use super::any_vec::AnyVec;
pub use super::page_index::PageIndex;

/// The rows of the entities that share one signature: one column per component.
///
/// A signature may span several semantic domains. See the book:
/// concepts/ecs.md, "The storage model".
pub struct ComponentPage {
    /// Each component's column, by its own [`ComponentKey`].
    pub(crate) columns: ColumnMap,

    /// The entity of each row: `entities[i]` owns row `i` of every column.
    pub(crate) entities: Vec<EntityId>,

    /// The page's signature: its component keys, sorted so it is canonical.
    pub(crate) keys: Vec<ComponentKey>,

    /// Each column's ticks, under the column's own key.
    pub(crate) ticks: TickMap,

    /// ω — moves on whenever a row changes place, never for a value written.
    pub(crate) order: u64,
}

impl ComponentPage {
    /// Adds an entity to this page's entity list, and a tick per column for its row.
    ///
    /// The caller pushes the row into every column, so each column keeps one
    /// row per entity.
    pub(crate) fn add_entity(
        &mut self,
        entity_id: EntityId,
        observed: &std::collections::HashSet<ComponentKey>,
        written_at: impl Fn(ComponentKey) -> Tick,
    ) {
        let rows_before = self.entities.len();
        self.entities.push(entity_id);
        for key in &self.keys {
            self.ticks.entry(*key).or_default().push(
                written_at(*key),
                rows_before,
                observed.contains(key),
            );
        }
    }

    /// Removes row `row_index` from every column, moving the last row into its place.
    ///
    /// The caller updates the metadata of the entity moved from the last row.
    pub(crate) fn swap_remove_row(&mut self, row_index: u32) {
        self.entities.swap_remove(row_index as usize);

        for column in self.columns.values_mut() {
            column.swap_remove_any(row_index as usize);
        }

        // The ticks follow their rows; the order moved, no value did.
        for ticks in self.ticks.values_mut() {
            ticks.swap_remove(row_index as usize);
        }
        self.order += 1;
    }

    /// Returns the number of rows this page stores.
    pub(crate) fn row_count(&self) -> usize {
        self.entities.len()
    }

    /// Gives the column `key` one tick per row, if it keeps none yet (see
    /// [`ColumnTicks::materialize`]).
    pub(crate) fn materialize_row_ticks(&mut self, key: ComponentKey) {
        let row_count = self.entities.len();
        if let Some(ticks) = self.ticks.get_mut(&key) {
            ticks.materialize(row_count);
        }
    }

    /// Returns the ticks of the column `key`, `None` when the page holds no such column.
    pub fn column_ticks(&self, key: ComponentKey) -> Option<&ColumnTicks> {
        self.ticks.get(&key)
    }

    /// Returns ω, the page's order version: it moves whenever a row changes place.
    pub fn order(&self) -> u64 {
        self.order
    }
}

/// A page's column ticks, by key, hashed like its columns.
pub(crate) type TickMap = std::collections::HashMap<
    ComponentKey,
    ColumnTicks,
    std::hash::BuildHasherDefault<crate::ecs::component_key::KeyHasher>,
>;
