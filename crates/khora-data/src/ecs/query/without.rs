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

//! `Without<T>`: the filter that excludes entities carrying `T`.

use khora_core::ecs::entity::EntityId;

use super::{live_metadata, WorldQuery};
use crate::ecs::{page::ComponentPage, Component, World};
use std::{any::TypeId, marker::PhantomData};

/// A `WorldQuery` filter that matches entities that do NOT have component `T`.
///
/// This is used as a marker in a query tuple to exclude entities. For example,
/// `Query<(&Position, Without<Velocity>)>` will iterate over all entities
/// that have a `Position` but no `Velocity`.
pub struct Without<T: Component>(PhantomData<T>);

// `Without<T>` itself doesn't fetch any data, so its `WorldQuery` implementation
// is mostly empty. It acts as a signal to the query engine.
impl<T: Component> WorldQuery for Without<T> {
    /// This query item is a zero-sized unit type, as it fetches no data.
    type Item<'a> = ();

    /// A `Without` filter does not add any component types to the query's main
    /// signature for page matching. The filtering is handled separately.
    fn type_ids() -> Vec<TypeId> {
        Vec::new() // Returns an empty Vec.
    }

    /// This is the key part of the filter: it returns the TypeId of the component to filter out.
    fn without_type_ids() -> Vec<TypeId> {
        // This is the key change: it returns the TypeId of the component to filter out.
        vec![TypeId::of::<T>()]
    }

    /// Fetches nothing. Returns a unit type `()`.
    unsafe fn fetch<'a>(_page_ptr: *const ComponentPage, _row_index: usize) -> Self::Item<'a> {
        // This function will be called but its result is ignored.
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: `fetch_from_world` is an `unsafe fn` whose contract requires
        // `world` to point to a `World` valid for `'a`; the `Query`/`QueryMut`
        // callers always pass a pointer derived from their live borrow.
        let world = unsafe { &*world };
        let metadata = live_metadata(world, entity_id)?;

        // Check ALL pages associated with this entity.
        // If ANY page contains the forbidden component, filtering failed.
        for location in metadata.locations.values() {
            let page = &world.storage.pages[location.page_id as usize];
            if page.type_ids.binary_search(&TypeId::of::<T>()).is_ok() {
                return None;
            }
        }
        Some(())
    }
}
