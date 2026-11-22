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
use crate::ecs::{AnyVec, ColumnOps, ColumnSnapshot, FieldWriteError, LossCause};

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

    /// Returns the fields this column's rows hold.
    pub(crate) fn layout(&self) -> &PackedLayout {
        &self.layout
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

    /// Returns the value of field `slot` in row `row`.
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
        let Some(accepted) = self.layout.accept(slot, value) else {
            return Err(FieldError {
                field: field.name.clone(),
                expected: field.kind,
                found: value.type_name().to_owned(),
            });
        };
        self.store(row, slot, accepted);
        Ok(())
    }

    /// Returns whether `slot` names a field and `row` a row.
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

    /// Returns the column rebuilt under `layout`, each field matched by name,
    /// with every value a row did not keep.
    ///
    /// Each value is read into its new field as a scene load reads it
    /// ([`PackedLayout::fit_from`]); one the field refuses takes the field's
    /// default, and is lost for the reason the field gives. A row loses
    /// nothing where its value is its new one.
    pub(crate) fn relaid(&self, layout: &Arc<PackedLayout>) -> (PackedColumn, Vec<RowLoss>) {
        let mut column = PackedColumn::new(layout.clone());
        // Each new field's slot in the old layout.
        let sources: Vec<Option<usize>> = layout
            .fields()
            .iter()
            .map(|field| self.layout.slot_of(&field.name))
            .collect();
        let dropped: Vec<usize> = (0..self.layout.fields().len())
            .filter(|old| layout.slot_of(&self.layout.fields()[*old].name).is_none())
            .collect();
        let mut losses = Vec::new();
        for row in 0..self.rows {
            column.push_defaults();
            for (slot, source) in sources.iter().enumerate() {
                let Some(old) = *source else {
                    continue;
                };
                let Some(was) = self.get(row, old) else {
                    continue;
                };
                let cause = match layout.fit_from(slot, &was, self.layout.ty(old)) {
                    Ok((value, _)) => {
                        column.store(row, slot, value);
                        continue;
                    }
                    Err(cause) => cause,
                };
                let now = column.get(row, slot);
                if now.as_ref() != Some(&was) {
                    losses.push(RowLoss {
                        row,
                        field: layout.fields()[slot].name.clone(),
                        was,
                        now,
                        cause,
                    });
                }
            }
            for &old in &dropped {
                if let Some(was) = self.get(row, old) {
                    losses.push(RowLoss {
                        row,
                        field: self.layout.fields()[old].name.clone(),
                        was,
                        now: None,
                        cause: LossCause::FieldDropped,
                    });
                }
            }
        }
        (column, losses)
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

/// One value a row of a [`PackedColumn`] did not keep through [`PackedColumn::relaid`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowLoss {
    /// The row, in the column's page.
    pub(crate) row: usize,
    /// The field, by name.
    pub(crate) field: String,
    /// The value the row held.
    pub(crate) was: ScriptValue,
    /// What the row holds in its place, `None` when the field is gone.
    pub(crate) now: Option<ScriptValue>,
    /// Why it was not kept.
    pub(crate) cause: LossCause,
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
            if self.layout.accept(slot, value).is_none() {
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
