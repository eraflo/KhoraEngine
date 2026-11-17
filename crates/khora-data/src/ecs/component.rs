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

//! The `Component` trait: data attached to an entity, and how its column is laid out.

use khora_core::script::{ErgonType, FieldValueError, ScriptValue};

use crate::ecs::page::AnyVec;

/// Data an entity can carry; implemented by `#[derive(Component)]`.
///
/// `Clone` lets a row be copied between pages when the entity changes shape.
/// The column hooks default to an array-of-structures `Vec<Self>`; a component
/// with `#[component(layout = "soa")]` overrides them all with its field-split
/// column. See the book: concepts/agdf.md,
/// "Layer 2 — the field-SoA substrate, and why it is the lever".
pub trait Component: Clone + 'static + Send + Sync {
    /// Creates this component's empty column.
    fn make_column() -> Box<dyn AnyVec>
    where
        Self: Sized,
    {
        Box::new(Vec::<Self>::new())
    }

    /// Appends `self` as a new row into its column.
    ///
    /// # Panics
    /// Panics if `column` is not this component's column type.
    fn push_into_column(self, column: &mut dyn AnyVec)
    where
        Self: Sized,
    {
        column
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch")
            .push(self);
    }

    /// Clones row `src_row` of `src` onto the end of `dst`, both this component's columns.
    ///
    /// # Panics
    /// Panics on a column type mismatch.
    fn copy_row_between(src: &dyn AnyVec, src_row: usize, dst: &mut dyn AnyVec)
    where
        Self: Sized,
    {
        let s = src
            .as_any()
            .downcast_ref::<Vec<Self>>()
            .expect("AoS column type mismatch");
        let dst = dst
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch");
        dst.push(s[src_row].clone());
    }

    /// Returns row `row` of `column` as an owned value, whatever the column's layout.
    ///
    /// # Panics
    /// Panics on a column type mismatch.
    fn clone_from_column(column: &dyn AnyVec, row: usize) -> Self
    where
        Self: Sized,
    {
        column
            .as_any()
            .downcast_ref::<Vec<Self>>()
            .expect("AoS column type mismatch")[row]
            .clone()
    }

    /// Overwrites row `row` of `column` with `self`, whatever the column's layout.
    ///
    /// # Panics
    /// Panics on a column type mismatch.
    fn set_in_column(self, column: &mut dyn AnyVec, row: usize)
    where
        Self: Sized,
    {
        let v = column
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch");
        v[row] = self;
    }

    /// Returns the names of the fields a script reaches; a field's slot is its index here.
    fn script_fields() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &[]
    }

    /// Returns the Ergon type of the field at `slot`, `None` for one a script cannot reach.
    fn script_type(_slot: usize) -> Option<ErgonType>
    where
        Self: Sized,
    {
        None
    }

    /// Returns the field at `slot`, `None` for one a script cannot reach.
    fn read_field(&self, _slot: usize) -> Option<ScriptValue> {
        None
    }

    /// Checks that the field at `slot` would take `value`, without writing it.
    ///
    /// The check [`write_field`](Self::write_field) makes, so a row can be
    /// checked whole before any field is written. Default: every value refused.
    ///
    /// # Errors
    ///
    /// Returns a [`FieldValueError`] when the field cannot hold `value`.
    fn check_field(_slot: usize, value: &ScriptValue) -> Result<(), FieldValueError> {
        Err(FieldValueError {
            expected: "a field a script can write".to_owned(),
            found: value.type_name().to_owned(),
        })
    }

    /// Writes the field at `slot`, refusing a value it cannot hold.
    ///
    /// Default: every write refused.
    ///
    /// # Errors
    ///
    /// Returns a [`FieldValueError`] when the field cannot hold `value`; the
    /// field is left as it was.
    fn write_field(&mut self, _slot: usize, value: &ScriptValue) -> Result<(), FieldValueError> {
        Err(FieldValueError {
            expected: "a field a script can write".to_owned(),
            found: value.type_name().to_owned(),
        })
    }
}
