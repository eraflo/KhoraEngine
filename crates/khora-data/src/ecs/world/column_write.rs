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

//! Many rows of one component, written in one page-grouped pass.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use std::sync::Arc;

use super::World;
use crate::ecs::ComponentKey;

/// What a [`World::write_column`] did with the rows it was given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnWriteReport {
    /// Rows written.
    pub written: usize,
    /// Rows skipped because their entity is gone or does not hold the
    /// component.
    pub missing: usize,
    /// Rows skipped because a field refused its value; nothing of such a row
    /// is written.
    pub refused: usize,
}

impl World {
    /// Writes, for every `(entity, values)` row, `values[i]` into the field
    /// at `slots[i]` of `entity`'s row of component `key`, grouped by page.
    /// A row of an entity that is gone, or holds no such component, is
    /// skipped; a row a field refuses is skipped whole; the others apply. Of
    /// two rows of one entity, the later is the one left written. The
    /// component's domain epoch is bumped once, when anything was written.
    pub fn write_column<'v>(
        &mut self,
        key: ComponentKey,
        slots: &[usize],
        rows: impl IntoIterator<Item = (EntityId, &'v [ScriptValue])>,
    ) -> ColumnWriteReport {
        let mut report = ColumnWriteReport::default();
        // No field to write: nothing changes, and nothing is said to.
        if slots.is_empty() {
            return report;
        }
        let Some(vtable) = self.storage.registry.vtable(key) else {
            report.missing = rows.into_iter().count();
            return report;
        };
        let (domain, ops) = (vtable.domain, Arc::clone(&vtable.columns));

        // Each row's place in the component's domain: the page, the row, and
        // the row of the batch. A gone or stale entity, or one with no row in
        // the domain, has none.
        let mut placed: Vec<(u32, u32, &'v [ScriptValue])> = Vec::new();
        for (entity, values) in rows {
            // A row holds one value a slot, or none of it is written.
            if values.len() != slots.len() {
                report.refused += 1;
                continue;
            }
            let location = self
                .entities
                .get(entity.index as usize)
                .filter(|(id, _)| *id == entity)
                .and_then(|(_, metadata)| metadata.as_ref()?.locations.get(&domain).copied());
            match location {
                Some(at) => placed.push((at.page_id, at.row_index, values)),
                None => report.missing += 1,
            }
        }
        // Page by page, the rows in memory order — a sweep, not random
        // accesses. Stable: of two rows of one entity, the later stays last.
        placed.sort_by_key(|(page, row, _)| (*page, *row));

        let mut start = 0;
        let mut run: Vec<(usize, &[ScriptValue])> = Vec::new();
        while start < placed.len() {
            let page = placed[start].0;
            let end = start
                + placed[start..]
                    .iter()
                    .take_while(|(p, ..)| *p == page)
                    .count();
            let Some(column) = self.storage.pages[page as usize].columns.get_mut(&key) else {
                // In the domain, without this component.
                report.missing += end - start;
                start = end;
                continue;
            };
            run.clear();
            run.extend(
                placed[start..end]
                    .iter()
                    .map(|(_, row, values)| (*row as usize, *values)),
            );
            let written = ops.write_rows(column.as_mut(), slots, &run);
            report.written += written;
            report.refused += run.len() - written;
            start = end;
        }
        if report.written > 0 {
            self.bump_domain_epoch(domain);
        }
        report
    }
}
