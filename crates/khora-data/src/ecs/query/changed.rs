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

//! `Changed<T>`: the filter that keeps the entities whose `T` was written
//! after the query's `since`.

use khora_core::ecs::entity::EntityId;

use super::{live_location, FetchContext, WorldQuery};
use crate::ecs::{page::ComponentPage, Component, ComponentKey, Tick, World};
use std::{any::TypeId, marker::PhantomData};

/// A `WorldQuery` filter that matches the entities holding a `T` written
/// after the query's [`since`](super::Query::since) tick.
///
/// It requires `T`, as `&T` does — an entity without `T` never matches —
/// and yields `()`. Per page of `T`'s column: a page whose summary is not
/// past `since` is skipped whole; a column keeping row ticks, read at or
/// above its floor, keeps the rows whose tick is past `since`; otherwise
/// every row is kept.
pub struct Changed<T: Component>(PhantomData<T>);

impl<T: Component> WorldQuery for Changed<T> {
    type Item<'a> = ();

    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    /// None: the filter reads `T`'s ticks, never its value, so it may stand
    /// beside a `&mut T` term.
    fn accessed_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn page_passes(page: &ComponentPage, since: Tick) -> bool {
        page.ticks
            .get(&ComponentKey::of::<T>())
            .is_none_or(|ticks| ticks.summary > since)
    }

    fn row_passes(page: &ComponentPage, row: usize, since: Tick) -> bool {
        page.ticks
            .get(&ComponentKey::of::<T>())
            .is_none_or(|ticks| ticks.changed_since(row, since))
    }

    unsafe fn fetch<'a>(
        _page_ptr: *const ComponentPage,
        _row_index: usize,
        _ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        // The rows that reach a fetch passed `row_passes` on their page.
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: the caller hands a `World` valid for `'a`; this only reads.
        let world = unsafe { &*world };
        let location = live_location::<T>(world, entity_id)?;
        let page = world.storage.pages.get(location.page_id as usize)?;
        page.columns.get(&ComponentKey::of::<T>())?;
        Self::row_passes(page, location.row_index as usize, ctx.since).then_some(())
    }
}
