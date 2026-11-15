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

use super::{live_metadata, FetchContext, WorldQuery};
use crate::ecs::{page::ComponentPage, Component, ComponentKey, World};
use std::{any::TypeId, marker::PhantomData};

/// A `WorldQuery` filter that matches entities that do NOT have component `T`.
///
/// For example, `world.query::<(&Position, Without<Velocity>)>()` yields every
/// entity that has a `Position` but no `Velocity`.
pub struct Without<T: Component>(PhantomData<T>);

impl<T: Component> WorldQuery for Without<T> {
    type Item<'a> = ();

    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn without_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    unsafe fn fetch<'a>(
        _page_ptr: *const ComponentPage,
        _row_index: usize,
        _ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        _ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: `fetch_from_world` is an `unsafe fn` whose contract requires
        // `world` to point to a `World` valid for `'a`; the `Query`/`QueryMut`
        // callers always pass a pointer derived from their live borrow.
        let world = unsafe { &*world };
        let metadata = live_metadata(world, entity_id)?;

        // `T` may sit in any of the entity's pages, whatever its domain.
        for location in metadata.locations.values() {
            let page = &world.storage.pages[location.page_id as usize];
            if page.keys.binary_search(&ComponentKey::of::<T>()).is_ok() {
                return None;
            }
        }
        Some(())
    }
}
