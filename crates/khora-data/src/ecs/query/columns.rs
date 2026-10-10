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

//! `Soa<T>`: a query over a field-split component, yielding its field arrays.

use khora_core::ecs::entity::EntityId;

use super::WorldQuery;
use crate::ecs::{
    page::{AnyVec, ComponentPage},
    ComponentKey, FieldSoaColumn, SoaLayout, World,
};
use std::{any::TypeId, marker::PhantomData};

/// A query that reads a field-SoA component **by value**.
///
/// A component stored field-SoA (`#[component(layout = "soa")]`) cannot be
/// borrowed as `&T` — its bytes are split across per-field lanes, not laid out
/// as a contiguous `T` — so it is queried as `Soa<T>`, which yields an owned
/// `T` reconstructed (gathered) from the lanes. For the bulk SIMD path that is
/// the *point* of the layout, use [`World::for_each_soa_column_mut`] instead;
/// `Soa<T>` is the per-row, mix-with-other-components access.
///
/// [`World::for_each_soa_column_mut`]: crate::ecs::World::for_each_soa_column_mut
pub struct Soa<T: SoaLayout>(PhantomData<T>);

impl<T: SoaLayout> WorldQuery for Soa<T> {
    type Item<'a> = T;

    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        let page = &*page_ptr;
        let column: &dyn AnyVec = &**page.columns.get(&ComponentKey::of::<T>()).unwrap();
        let soa = column
            .as_any()
            .downcast_ref::<FieldSoaColumn<T>>()
            .expect("Soa<T> queried on a component that is not field-SoA stored");
        soa.get(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        let world = &*world;
        world.clone_component::<T>(entity_id)
    }
}
