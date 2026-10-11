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

//! What storage does with a component's columns, whose type it never knows.

use std::marker::PhantomData;

pub use khora_core::script::ColumnSnapshot;
use khora_core::script::{FieldValueError, ScriptValue};

use crate::ecs::packed::PackedLayout;
use crate::ecs::{AnyVec, Component};

/// The column operations of a component's
/// [`ComponentVTable`](crate::ecs::ComponentVTable).
///
/// Pages hold type-erased columns (`Box<dyn AnyVec>`); creating a column or
/// moving a row between pages goes through these operations, so storage never
/// knows a column's concrete type. A trait object rather than `fn` pointers so
/// the operations can carry data: a declared component's columns need its
/// fields. A Rust component's carry none — one zero-sized `RustColumns<T>` per
/// type.
pub trait ColumnOps: Send + Sync {
    /// A new, empty column for this component.
    fn create_column(&self) -> Box<dyn AnyVec>;

    /// Copies row `row` of `src` onto the end of `dst`, both columns of this
    /// component.
    fn copy_row(&self, src: &dyn AnyVec, row: usize, dst: &mut dyn AnyVec);

    /// The size of one row, in bytes.
    fn size_bytes(&self) -> usize;

    /// The fields of a declared component, `None` for a Rust one.
    fn packed(&self) -> Option<&PackedLayout> {
        None
    }

    /// The slot of the field a script names `name`, if the component has one.
    fn field_slot(&self, name: &str) -> Option<usize>;

    /// The Ergon type of the field at `slot`, `None` for one a script cannot
    /// reach. Default: none.
    fn field_type(&self, _slot: usize) -> Option<khora_core::script::ErgonType> {
        None
    }

    /// The value of the field at `slot` of row `row` of `column`, `None` for a
    /// field a script cannot reach.
    fn read_field(&self, column: &dyn AnyVec, row: usize, slot: usize) -> Option<ScriptValue>;

    /// Writes the fields of row `row` of `column` the patch names, by slot.
    /// All or nothing: every value is checked before any is written.
    fn write_fields(
        &self,
        column: &mut dyn AnyVec,
        row: usize,
        patch: &[(usize, &ScriptValue)],
    ) -> Result<(), FieldWriteError>;

    /// A copy of the column, readable row by row without the `World`.
    fn snapshot(&self, column: &dyn AnyVec) -> Box<dyn ColumnSnapshot>;

    /// Writes many rows of `column`: for each `(row, values)`, `values[i]`
    /// into the field at `slots[i]`. Each row is all or nothing, as
    /// [`Self::write_fields`] is; a refused row is skipped and the others are
    /// written. Returns how many rows were written.
    fn write_rows(
        &self,
        column: &mut dyn AnyVec,
        slots: &[usize],
        rows: &[(usize, &[ScriptValue])],
    ) -> usize {
        let mut patch = Vec::with_capacity(slots.len());
        let mut written = 0;
        for &(row, values) in rows {
            patch.clear();
            patch.extend(slots.iter().copied().zip(values.iter()));
            if self.write_fields(column, row, &patch).is_ok() {
                written += 1;
            }
        }
        written
    }
}

/// Why a typed write to a component's fields was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldWriteError {
    /// The component has no field of that name.
    NoSuchField(String),
    /// The field refused the value.
    Refused {
        /// The field.
        field: String,
        /// Why.
        error: FieldValueError,
    },
    /// The field exists, but its type is not one a script can write.
    NotAccessible(String),
}

/// The column operations of a Rust component: its own column hooks, so a
/// field-SoA component keeps its `FieldSoaColumn`.
pub(crate) struct RustColumns<T: Component>(PhantomData<fn() -> T>);

impl<T: Component> RustColumns<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: Component> ColumnOps for RustColumns<T> {
    fn create_column(&self) -> Box<dyn AnyVec> {
        T::make_column()
    }

    fn copy_row(&self, src: &dyn AnyVec, row: usize, dst: &mut dyn AnyVec) {
        T::copy_row_between(src, row, dst);
    }

    fn size_bytes(&self) -> usize {
        std::mem::size_of::<T>()
    }

    fn field_slot(&self, name: &str) -> Option<usize> {
        T::script_fields().iter().position(|field| *field == name)
    }

    fn field_type(&self, slot: usize) -> Option<khora_core::script::ErgonType> {
        T::script_type(slot)
    }

    fn read_field(&self, column: &dyn AnyVec, row: usize, slot: usize) -> Option<ScriptValue> {
        if row >= column.len() {
            return None;
        }
        // An array-of-structs column is read in place; a field-split one
        // gathers its row first.
        match column.as_any().downcast_ref::<Vec<T>>() {
            Some(rows) => rows[row].read_field(slot),
            None => T::clone_from_column(column, row).read_field(slot),
        }
    }

    fn write_fields(
        &self,
        column: &mut dyn AnyVec,
        row: usize,
        patch: &[(usize, &ScriptValue)],
    ) -> Result<(), FieldWriteError> {
        let names = T::script_fields();
        for &(slot, _) in patch {
            let Some(name) = names.get(slot) else {
                return Err(FieldWriteError::NoSuchField(format!("#{slot}")));
            };
            if T::script_type(slot).is_none() {
                return Err(FieldWriteError::NotAccessible((*name).to_owned()));
            }
        }
        // Written into a copy of the row, swapped in only once every field
        // took its value: a refused value leaves the row as it was.
        let mut value = T::clone_from_column(column, row);
        for &(slot, written) in patch {
            value
                .write_field(slot, written)
                .map_err(|error| FieldWriteError::Refused {
                    field: names[slot].to_owned(),
                    error,
                })?;
        }
        value.set_in_column(column, row);
        Ok(())
    }

    /// In place where the column is an array of `T`: each row's values are
    /// checked whole ([`Component::check_field`]), then written where they
    /// sit — no copy of a row is made, and a refused row is left untouched. A
    /// field-split column takes the copy road of [`Self::write_fields`].
    fn write_rows(
        &self,
        column: &mut dyn AnyVec,
        slots: &[usize],
        rows: &[(usize, &[ScriptValue])],
    ) -> usize {
        // The slots are the batch's, checked once rather than once a row.
        if slots.iter().any(|&slot| T::script_type(slot).is_none()) {
            return 0;
        }
        let Some(values) = column.as_any_mut().downcast_mut::<Vec<T>>() else {
            let mut patch = Vec::with_capacity(slots.len());
            let mut written = 0;
            for &(row, row_values) in rows {
                patch.clear();
                patch.extend(slots.iter().copied().zip(row_values.iter()));
                if self.write_fields(column, row, &patch).is_ok() {
                    written += 1;
                }
            }
            return written;
        };
        let mut written = 0;
        for &(row, row_values) in rows {
            let Some(target) = values.get_mut(row) else {
                continue;
            };
            let fits = slots
                .iter()
                .zip(row_values)
                .all(|(&slot, value)| T::check_field(slot, value).is_ok());
            if !fits {
                continue;
            }
            for (&slot, value) in slots.iter().zip(row_values) {
                // Checked above: the value is one the field takes.
                let _ = target.write_field(slot, value);
            }
            written += 1;
        }
        written
    }

    fn snapshot(&self, column: &dyn AnyVec) -> Box<dyn ColumnSnapshot> {
        let rows = match column.as_any().downcast_ref::<Vec<T>>() {
            Some(rows) => rows.clone(),
            None => (0..column.len())
                .map(|row| T::clone_from_column(column, row))
                .collect(),
        };
        Box::new(RustSnapshot { rows })
    }
}

/// A typed copy of a Rust component's column; a row becomes a script value
/// only when it is read.
struct RustSnapshot<T: Component> {
    rows: Vec<T>,
}

impl<T: Component> ColumnSnapshot for RustSnapshot<T> {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn read(&self, row: usize) -> ScriptValue {
        let value = &self.rows[row];
        ScriptValue::Struct(
            T::script_fields()
                .iter()
                .enumerate()
                .filter_map(|(slot, name)| Some(((*name).to_owned(), value.read_field(slot)?)))
                .collect(),
        )
    }
}
