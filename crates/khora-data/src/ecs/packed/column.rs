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

//! The column of a declared component, and its column operations.

use std::any::Any;
use std::sync::Arc;

use khora_core::script::{FieldValueError, ScriptValue};

use super::{decode, encode, FieldError, PackedLayout, Place};
use crate::ecs::{AnyVec, ColumnOps, ColumnSnapshot, FieldWriteError};

/// The rows of one run-time component in one page: the inline parts packed
/// back to back, `stride` bytes each, and the out-of-line values beside them.
#[derive(Clone)]
pub(crate) struct PackedColumn {
    layout: Arc<PackedLayout>,
    bytes: Vec<u8>,
    boxed: Vec<ScriptValue>,
    rows: usize,
}

impl PackedColumn {
    pub(crate) fn new(layout: Arc<PackedLayout>) -> Self {
        Self {
            layout,
            bytes: Vec::new(),
            boxed: Vec::new(),
            rows: 0,
        }
    }

    /// Appends a row holding every field's default.
    pub(crate) fn push_defaults(&mut self) {
        self.bytes
            .resize(self.bytes.len() + self.layout.stride(), 0);
        self.boxed
            .resize(self.boxed.len() + self.layout.boxed(), ScriptValue::Unit);
        self.rows += 1;
        let row = self.rows - 1;
        for slot in 0..self.layout.fields().len() {
            let default = self.layout.fields()[slot].default.clone();
            self.store(row, slot, default);
        }
    }

    /// The value of field `slot` in row `row`.
    pub(crate) fn get(&self, row: usize, slot: usize) -> Option<ScriptValue> {
        if row >= self.rows {
            return None;
        }
        let field = self.layout.fields().get(slot)?;
        Some(match self.layout.place(slot)? {
            Place::Inline(offset) => {
                let start = row * self.layout.stride() + offset;
                decode(field.kind, &self.bytes[start..])
            }
            Place::Boxed(index) => self.boxed[row * self.layout.boxed() + index].clone(),
        })
    }

    /// Writes field `slot` of row `row`, refusing a value not of its kind.
    pub(crate) fn set(
        &mut self,
        row: usize,
        slot: usize,
        value: &ScriptValue,
    ) -> Result<(), FieldError> {
        let field = &self.layout.fields()[slot];
        let Some(accepted) = field.kind.accept(value) else {
            return Err(FieldError {
                field: field.name.clone(),
                expected: field.kind,
                found: value.type_name().to_owned(),
            });
        };
        self.store(row, slot, accepted);
        Ok(())
    }

    /// Whether `slot` names a field and `row` a row.
    pub(crate) fn has(&self, row: usize, slot: usize) -> bool {
        row < self.rows && slot < self.layout.fields().len()
    }

    /// Stores `value`, already checked of its field's kind.
    fn store(&mut self, row: usize, slot: usize, value: ScriptValue) {
        match self.layout.place(slot) {
            Some(Place::Inline(offset)) => {
                let start = row * self.layout.stride() + offset;
                encode(&value, &mut self.bytes[start..]);
            }
            Some(Place::Boxed(index)) => {
                self.boxed[row * self.layout.boxed() + index] = value;
            }
            None => {}
        }
    }

    /// Appends row `row` of `src`, the same component's column — a deep copy:
    /// the out-of-line values are cloned with the row.
    fn push_copy_of(&mut self, src: &PackedColumn, row: usize) {
        let stride = self.layout.stride();
        self.bytes
            .extend_from_slice(&src.bytes[row * stride..(row + 1) * stride]);
        let boxed = self.layout.boxed();
        self.boxed
            .extend_from_slice(&src.boxed[row * boxed..(row + 1) * boxed]);
        self.rows += 1;
    }

    /// The column rebuilt under `layout`: each field kept by name where its
    /// kind is unchanged, its new default otherwise.
    pub(crate) fn relaid(&self, layout: &Arc<PackedLayout>) -> PackedColumn {
        let mut column = PackedColumn::new(layout.clone());
        let kept: Vec<Option<usize>> = layout
            .fields()
            .iter()
            .map(|field| {
                let old = self.layout.slot_of(&field.name)?;
                (self.layout.fields()[old].kind == field.kind).then_some(old)
            })
            .collect();
        for row in 0..self.rows {
            column.push_defaults();
            for (slot, old) in kept.iter().enumerate() {
                if let Some(value) = old.and_then(|old| self.get(row, old)) {
                    column.store(row, slot, value);
                }
            }
        }
        column
    }
}

impl AnyVec for PackedColumn {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn swap_remove_any(&mut self, index: usize) {
        let last = self.rows - 1;
        let stride = self.layout.stride();
        if index != last {
            self.bytes
                .copy_within(last * stride..(last + 1) * stride, index * stride);
            let boxed = self.layout.boxed();
            for k in 0..boxed {
                self.boxed.swap(index * boxed + k, last * boxed + k);
            }
        }
        self.bytes.truncate(last * stride);
        self.boxed.truncate(last * self.layout.boxed());
        self.rows = last;
    }

    fn len(&self) -> usize {
        self.rows
    }
}

/// The column operations of a declared component: its columns are [`PackedColumn`]s of
/// its layout.
pub(crate) struct PackedColumns {
    layout: Arc<PackedLayout>,
}

impl PackedColumns {
    pub(crate) fn new(layout: Arc<PackedLayout>) -> Self {
        Self { layout }
    }
}

impl ColumnOps for PackedColumns {
    fn create_column(&self) -> Box<dyn AnyVec> {
        Box::new(PackedColumn::new(self.layout.clone()))
    }

    fn copy_row(&self, src: &dyn AnyVec, row: usize, dst: &mut dyn AnyVec) {
        let src = src
            .as_any()
            .downcast_ref::<PackedColumn>()
            .expect("a run-time component's column is packed");
        dst.as_any_mut()
            .downcast_mut::<PackedColumn>()
            .expect("a run-time component's column is packed")
            .push_copy_of(src, row);
    }

    fn size_bytes(&self) -> usize {
        self.layout.stride() + self.layout.boxed() * std::mem::size_of::<ScriptValue>()
    }

    fn packed(&self) -> Option<&PackedLayout> {
        Some(&self.layout)
    }

    fn field_slot(&self, name: &str) -> Option<usize> {
        self.layout.slot_of(name)
    }

    fn read_field(&self, column: &dyn AnyVec, row: usize, slot: usize) -> Option<ScriptValue> {
        column
            .as_any()
            .downcast_ref::<PackedColumn>()?
            .get(row, slot)
    }

    fn write_fields(
        &self,
        column: &mut dyn AnyVec,
        row: usize,
        patch: &[(usize, &ScriptValue)],
    ) -> Result<(), FieldWriteError> {
        // Every value checked before any is written: all or nothing.
        for &(slot, value) in patch {
            let Some(field) = self.layout.fields().get(slot) else {
                return Err(FieldWriteError::NoSuchField(format!("#{slot}")));
            };
            if field.kind.accept(value).is_none() {
                return Err(FieldWriteError::Refused {
                    field: field.name.clone(),
                    error: FieldValueError {
                        expected: field.kind.spelling().to_owned(),
                        found: value.type_name().to_owned(),
                    },
                });
            }
        }
        let column = column
            .as_any_mut()
            .downcast_mut::<PackedColumn>()
            .expect("a declared component's column is packed");
        for &(slot, value) in patch {
            // Checked of its kind above.
            let _ = column.set(row, slot, value);
        }
        Ok(())
    }

    fn snapshot(&self, column: &dyn AnyVec) -> Box<dyn ColumnSnapshot> {
        let column = column
            .as_any()
            .downcast_ref::<PackedColumn>()
            .expect("a declared component's column is packed");
        Box::new(column.clone())
    }
}

/// A copy of a declared component's column.
impl ColumnSnapshot for PackedColumn {
    fn len(&self) -> usize {
        self.rows
    }

    fn read(&self, row: usize) -> ScriptValue {
        ScriptValue::Struct(
            self.layout
                .fields()
                .iter()
                .enumerate()
                .filter_map(|(slot, field)| Some((field.name.clone(), self.get(row, slot)?)))
                .collect(),
        )
    }
}
