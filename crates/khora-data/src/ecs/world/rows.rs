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

//! Reading and writing one entity's row of a component by key, whatever
//! the component — the road scripts and tools take.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::World;
use crate::ecs::packed::{FieldError, FieldKind, PackedColumn};
use crate::ecs::page::PageIndex;
use crate::ecs::{AnyVec, ColumnTicks, ComponentKey, ComponentVTable, FieldWriteError, Tick};

/// One entity's row of one component, read by id.
pub struct RowRef<'w> {
    vtable: &'w ComponentVTable,
    column: &'w dyn AnyVec,
    row: usize,
}

impl<'w> RowRef<'w> {
    /// The component.
    pub fn key(&self) -> ComponentKey {
        self.vtable.key
    }

    /// The component's vtable.
    pub fn vtable(&self) -> &'w ComponentVTable {
        self.vtable
    }

    /// The value of the field at `slot`, `None` for one a script cannot
    /// reach.
    pub fn field(&self, slot: usize) -> Option<ScriptValue> {
        self.vtable.columns.read_field(self.column, self.row, slot)
    }
}

/// One entity's row of one component, written by id. A write accepted
/// stamps the row with the clock's tick; a write refused is not a write.
pub struct RowMut<'w> {
    vtable: &'w ComponentVTable,
    column: &'w mut dyn AnyVec,
    ticks: &'w mut ColumnTicks,
    row: usize,
    tick: Tick,
    /// Whether the column is observed: it gains row ticks, if it keeps none,
    /// on the first write accepted.
    observed: bool,
    /// The page's row count, which those row ticks number.
    rows: usize,
    /// The change epoch of the component's domain, raised with the write.
    epoch: &'w mut u64,
}

impl<'w> RowMut<'w> {
    /// The component.
    pub fn key(&self) -> ComponentKey {
        self.vtable.key
    }

    /// The component's vtable.
    pub fn vtable(&self) -> &ComponentVTable {
        self.vtable
    }

    /// The value of the field at `slot`, `None` for one a script cannot
    /// reach.
    pub fn field(&self, slot: usize) -> Option<ScriptValue> {
        self.vtable
            .columns
            .read_field(&*self.column, self.row, slot)
    }

    /// Writes the fields the patch names, by slot — all or nothing: every
    /// value is checked before any is written.
    pub fn write_fields(&mut self, patch: &[(usize, &ScriptValue)]) -> Result<(), FieldWriteError> {
        self.vtable
            .columns
            .write_fields(self.column, self.row, patch)?;
        self.stamp();
        Ok(())
    }

    /// Writes the field at `slot`, refusing a value it cannot hold.
    ///
    /// # Panics
    ///
    /// When `slot` is not a field of the component: a slot comes from the
    /// component the row is read with.
    pub fn set_field(&mut self, slot: usize, value: &ScriptValue) -> Result<(), FieldError> {
        self.write_field(slot, value)?;
        self.stamp();
        Ok(())
    }

    /// Notes a write of the row.
    fn stamp(&mut self) {
        if self.observed {
            self.ticks.materialize(self.rows);
        }
        self.ticks.stamp(self.row, self.tick);
        *self.epoch = (*self.epoch).max(self.tick.0);
    }

    /// [`set_field`](Self::set_field), the row left unstamped.
    fn write_field(&mut self, slot: usize, value: &ScriptValue) -> Result<(), FieldError> {
        let row = self.row;
        if let Some(column) = self.column.as_any_mut().downcast_mut::<PackedColumn>() {
            assert!(
                column.has(row, slot),
                "slot {slot} is not a field of this row"
            );
            return column.set(row, slot, value);
        }
        let columns = &self.vtable.columns;
        let kind = columns
            .field_type(slot)
            .map_or(FieldKind::Value, |ty| FieldKind::of(&ty));
        match columns.write_fields(self.column, row, &[(slot, value)]) {
            Ok(()) => Ok(()),
            Err(FieldWriteError::NoSuchField(field)) => {
                panic!("slot {field} is not a field of this row")
            }
            Err(FieldWriteError::NotAccessible(field)) => Err(FieldError {
                field,
                expected: kind,
                found: value.type_name().to_owned(),
            }),
            Err(FieldWriteError::Refused { field, .. }) => Err(FieldError {
                field,
                expected: kind,
                found: value.type_name().to_owned(),
            }),
        }
    }
}

impl World {
    /// `entity`'s row of component `key`.
    pub fn row(&self, entity: EntityId, key: ComponentKey) -> Option<RowRef<'_>> {
        let (vtable, location) = self.row_location(entity, key)?;
        let column = self.storage.pages[location.page_id as usize]
            .columns
            .get(&key)?;
        Some(RowRef {
            vtable,
            column: column.as_ref(),
            row: location.row_index as usize,
        })
    }

    /// `entity`'s row of component `key`, writable. A write accepted through it
    /// stamps the row and raises the domain's change epoch; taking it changes
    /// nothing.
    pub fn row_mut(&mut self, entity: EntityId, key: ComponentKey) -> Option<RowMut<'_>> {
        let (vtable, location) = self.row_location(entity, key)?;
        let domain = vtable.domain;
        if !self.storage.pages[location.page_id as usize]
            .columns
            .contains_key(&key)
        {
            return None;
        }
        let observed = self.clock.observed.contains(&key);
        let tick = self.clock.tick;
        let epoch = &mut self.domain_epochs[domain.index()];
        let storage = &mut self.storage;
        let vtable = storage.registry.vtable(key)?;
        let page = &mut storage.pages[location.page_id as usize];
        let rows = page.row_count();
        let column = page.columns.get_mut(&key)?;
        Some(RowMut {
            vtable,
            column: column.as_mut(),
            ticks: page.ticks.entry(key).or_default(),
            row: location.row_index as usize,
            tick,
            observed,
            rows,
            epoch,
        })
    }

    /// The vtable of `key` and `entity`'s live row in its domain.
    fn row_location(
        &self,
        entity: EntityId,
        key: ComponentKey,
    ) -> Option<(&ComponentVTable, PageIndex)> {
        let vtable = self.storage.registry.vtable(key)?;
        let (slot, metadata) = self.entities.get(entity.index as usize)?;
        if *slot != entity {
            return None;
        }
        let location = *metadata.as_ref()?.locations.get(&vtable.domain)?;
        Some((vtable, location))
    }
}
