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
}
