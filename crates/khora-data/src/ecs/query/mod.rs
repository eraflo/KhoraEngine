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

use khora_core::ecs::entity::EntityId;

use crate::ecs::{
    entity::EntityMetadata,
    page::{AnyVec, ComponentPage, PageIndex},
    Component, ComponentKey, Mut, SemanticDomain, Tick, World,
};
use std::any::TypeId;

mod changed;
mod columns;
mod fetch_context;
mod native_row_plan;
mod read;
mod without;
mod write;

pub use changed::Changed;
pub use columns::Soa;
pub use fetch_context::FetchContext;
pub(crate) use fetch_context::Writable;
pub(crate) use native_row_plan::NativeRowPlan;
pub use read::Query;
pub use without::Without;
pub use write::QueryMut;

/// Returns `true` if `row` of `page_id` is the live entity's row for `domain`.
///
/// A migration leaves the old row in place until compaction, and its page
/// still matches the query: only the row the metadata points to is real. A
/// dead entity's row is never live, even once its index is recycled.
fn is_live_row(
    world: &World,
    page_id: u32,
    row: usize,
    entity: EntityId,
    domain: SemanticDomain,
) -> bool {
    live_metadata(world, entity)
        .and_then(|meta| meta.locations.get(&domain))
        .is_some_and(|loc| loc.page_id == page_id && loc.row_index as usize == row)
}

/// Returns `entity`'s metadata, or `None` if it is dead, even with its index recycled.
fn live_metadata(world: &World, entity: EntityId) -> Option<&EntityMetadata> {
    world
        .entities
        .get(entity.index as usize)
        .filter(|(slot, _)| slot.generation == entity.generation)
        .and_then(|(_, meta)| meta.as_ref())
}

/// Returns the live location of `entity`'s row in `T`'s domain.
///
/// `None` if the entity is dead, `T` is unregistered, or it has no row there.
fn live_location<T: 'static>(world: &World, entity: EntityId) -> Option<PageIndex> {
    let domain = world.storage.registry.get_domain(TypeId::of::<T>())?;
    live_metadata(world, entity)?
        .locations
        .get(&domain)
        .copied()
}

/// Returns `true` if the live `entity` holds any of `filters`' types, each in its domain's row.
///
/// The per-row half of a Native `Without` filter outside the driver domain
/// (see [`NativeRowPlan::foreign_without`]), which page exclusion cannot see.
fn has_foreign_excluded(
    world: &World,
    entity: EntityId,
    filters: &[(SemanticDomain, TypeId)],
) -> bool {
    let Some(meta) = live_metadata(world, entity) else {
        return false;
    };
    filters.iter().any(|(domain, type_id)| {
        meta.locations.get(domain).is_some_and(|loc| {
            world.storage.pages[loc.page_id as usize]
                .keys
                .binary_search(&ComponentKey::Rust(*type_id))
                .is_ok()
        })
    })
}

/// A term of a `World` query: a component reference, `EntityId`, a filter, or a tuple of terms.
///
/// See the book: concepts/ecs.md, "Queries and SoA versus AoSoA".
pub trait WorldQuery {
    /// The item the query yields, such as `(&'a Position, Mut<'a, Velocity>)`.
    type Item<'a>;

    /// Returns the sorted components a matching page must hold.
    fn type_ids() -> Vec<TypeId>;

    /// Returns every component the items reach, once per term that reaches it.
    ///
    /// Not deduplicated, so a query naming one component twice can be told apart.
    fn accessed_type_ids() -> Vec<TypeId> {
        let mut ids = Self::type_ids();
        ids.extend(Self::optional_type_ids());
        ids
    }

    /// Returns the sorted components a matching page must not hold.
    fn without_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Returns the components read by `Option` terms.
    ///
    /// They do not constrain which pages match; their domains say which page to read them from.
    fn optional_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Returns the components this query writes (`&mut T`, `Option<&mut T>`).
    ///
    /// [`World::query_mut`] makes them the writable terms, whose guards stamp what they write.
    fn mutable_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Returns `false` if no row of `page` can pass this query's filters.
    ///
    /// Asked of a Native iteration's driver pages only; a term without a
    /// column on the page passes it.
    fn page_passes(_page: &ComponentPage, _since: Tick) -> bool {
        true
    }

    /// Returns whether row `row` of `page` passes this query's filters, judged from the page alone.
    ///
    /// Asked of a Native iteration's live driver rows only; elsewhere
    /// `fetch_from_world` judges.
    fn row_passes(_page: &ComponentPage, _row: usize, _since: Tick) -> bool {
        true
    }

    /// Fetches the query's item from row `row_index` of a page, with `ctx` (see [`FetchContext`]).
    ///
    /// # Safety
    ///
    /// The caller (the query iterator) guarantees that:
    /// 1. The page pointed to by `page_ptr` is valid and matches the query's signature.
    /// 2. `row_index` is a valid index within the page's columns.
    /// 3. Aliasing rules are not violated (e.g., no two `&mut T` to the same data).
    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        ctx: FetchContext<'_>,
    ) -> Self::Item<'a>;

    /// Fetches the query's item directly from the world for a specific entity;
    /// a write through a writable item carries the world's current tick.
    ///
    /// # Safety
    /// Caller must ensure `world` is valid and no aliasing rules are violated.
    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        _ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>>;
}

impl<T: Component> WorldQuery for &T {
    type Item<'a> = &'a T;

    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    /// Fetches a reference to the component `T` from the specified row.
    ///
    /// # Safety
    /// The caller MUST guarantee that the page contains a column for component `T`
    /// and that `row_index` is in bounds.
    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        _ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        // SAFETY: the caller guarantees `page_ptr` points to a valid page.
        let page = unsafe { &*page_ptr };
        let column: &dyn AnyVec = &**page
            .columns
            .get(&ComponentKey::of::<T>())
            .expect("the caller guarantees the page holds a `T` column");
        let vec: &Vec<T> = column
            .as_any()
            .downcast_ref::<Vec<T>>()
            .expect("the column keyed by `T` holds `T`s");
        // SAFETY: the caller guarantees `row_index` is in bounds.
        unsafe { vec.get_unchecked(row_index) }
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        _ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: the caller guarantees `world` is valid.
        let world = unsafe { &*world };
        let location = live_location::<T>(world, entity_id)?;
        let page = &world.storage.pages[location.page_id as usize];
        let column = page.columns.get(&ComponentKey::of::<T>())?;
        let vec = column.as_any().downcast_ref::<Vec<T>>()?;
        vec.get(location.row_index as usize)
    }
}

impl<T: Component> WorldQuery for &mut T {
    type Item<'a> = Mut<'a, T>;

    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    fn mutable_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    /// Fetches writable access to the component `T` at the specified row: a
    /// guard that stamps the row on its first write.
    ///
    /// # Safety
    /// The caller MUST guarantee that:
    /// 1. The page contains a column for component `T`.
    /// 2. `row_index` is in bounds.
    /// 3. No other mutable reference to this specific component exists at the same time.
    ///    The query engine must enforce this.
    /// 4. `page_ptr` keeps write permission over the page for `'a` — it is
    ///    taken from the page buffer, not cast from a reference to one page —
    ///    and no reference to the page's ticks is live while an item is.
    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        // The cast to a mutable pointer holds only because the query engine
        // guarantees no other access to the row.
        let page_ptr = page_ptr as *mut ComponentPage;
        // SAFETY: the caller guarantees `page_ptr` is valid and keeps write
        // permission over the page for `'a`.
        let page = unsafe { &mut *page_ptr };
        let column = page
            .columns
            .get_mut(&ComponentKey::of::<T>())
            .expect("the caller guarantees the page holds a `T` column");
        let vec = column
            .as_any_mut()
            .downcast_mut::<Vec<T>>()
            .expect("the column keyed by `T` holds `T`s");
        debug_assert!(row_index < vec.len());
        // Through the buffer's pointer, never a slice: `get_unchecked_mut`
        // reborrows the whole column `&mut`, which invalidates every item an
        // earlier call handed out from it.
        // SAFETY: the caller guarantees `row_index` is in bounds.
        let value = unsafe { vec.as_mut_ptr().add(row_index) };
        let key = ComponentKey::of::<T>();
        let (observed, epoch) = ctx.writable(key);
        // SAFETY: `value` and `page_ptr` point into the same live page, and the
        // caller guarantees no other reference to this item exists.
        unsafe { Mut::in_page(value, page_ptr, key, observed, epoch, row_index, ctx.tick) }
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: the caller guarantees `world` is valid.
        let location = live_location::<T>(unsafe { &*world }, entity_id)?;
        let key = ComponentKey::of::<T>();
        let (tick, (observed, epoch)) = (ctx.tick, ctx.writable(key));
        // SAFETY: the caller keeps write permission over the world, and no
        // reference to the page is live.
        let page = unsafe { page_of_world(world, location.page_id) }?;
        let row = location.row_index as usize;
        // SAFETY: `page` points into the world's page buffer, checked in bounds.
        let column = unsafe { (*page).columns.get_mut(&ComponentKey::of::<T>()) }?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;
        // SAFETY: the caller guarantees no other reference to this item.
        let value = unsafe { column_item(vec, row) }?;
        // SAFETY: `value` and `page` point into the same live page.
        Some(unsafe { Mut::in_page(value, page, key, observed, epoch, row, tick) })
    }
}

impl<T: Component> WorldQuery for Option<&T> {
    type Item<'a> = Option<&'a T>;

    fn type_ids() -> Vec<TypeId> {
        // An optional term does not narrow which pages match.
        Vec::new()
    }

    fn optional_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        _ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        // SAFETY: the caller guarantees `page_ptr` points to a valid page.
        let page = unsafe { &*page_ptr };
        let column = page.columns.get(&ComponentKey::of::<T>())?;
        let vec = column.as_any().downcast_ref::<Vec<T>>()?;
        vec.get(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        _ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // SAFETY: the caller guarantees `world` is valid.
        let world = unsafe { &*world };
        // A dead entity fails the join; a live one lacking `T` — no row in
        // `T`'s domain, or no `T` column in that row's page — yields `Some(None)`.
        live_metadata(world, entity_id)?;
        let Some(location) = live_location::<T>(world, entity_id) else {
            return Some(None);
        };

        let page = &world.storage.pages[location.page_id as usize];
        Some(
            page.columns
                .get(&ComponentKey::of::<T>())
                .and_then(|column| column.as_any().downcast_ref::<Vec<T>>())
                .and_then(|vec| vec.get(location.row_index as usize)),
        )
    }
}

impl<T: Component> WorldQuery for Option<&mut T> {
    type Item<'a> = Option<Mut<'a, T>>;

    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn optional_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    fn mutable_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    /// # Safety
    /// As `&mut T`'s, the column aside: the page may lack it.
    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        let page_ptr = page_ptr as *mut ComponentPage;
        // SAFETY: the caller guarantees `page_ptr` is valid and keeps write
        // permission over the page for `'a`.
        let page = unsafe { &mut *page_ptr };
        let key = ComponentKey::of::<T>();
        let column = page.columns.get_mut(&key)?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;
        // SAFETY: the caller guarantees no other reference to this item.
        let value = unsafe { column_item(vec, row_index) }?;
        let (observed, epoch) = ctx.writable(key);
        // SAFETY: `value` and `page_ptr` point into the same live page.
        Some(unsafe { Mut::in_page(value, page_ptr, key, observed, epoch, row_index, ctx.tick) })
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
        ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        // Same contract as `Option<&T>`: a dead entity fails the join, a live
        // one lacking `T` yields `Some(None)`.
        // SAFETY: the caller guarantees `world` is valid.
        let world_ref = unsafe { &*world };
        live_metadata(world_ref, entity_id)?;
        let Some(location) = live_location::<T>(world_ref, entity_id) else {
            return Some(None);
        };
        let key = ComponentKey::of::<T>();
        let (tick, (observed, epoch)) = (ctx.tick, ctx.writable(key));
        // SAFETY: the caller keeps write permission over the world, and no
        // reference to the page is live.
        let Some(page) = (unsafe { page_of_world(world, location.page_id) }) else {
            return Some(None);
        };
        let row = location.row_index as usize;
        // SAFETY: `page` points into the world's page buffer, checked in bounds.
        let columns = unsafe { &mut (*page).columns };
        Some(
            columns
                .get_mut(&ComponentKey::of::<T>())
                .and_then(|column| column.as_any_mut().downcast_mut::<Vec<T>>())
                // SAFETY: the caller guarantees no other reference to this item.
                .and_then(|vec| unsafe { column_item(vec, row) })
                // SAFETY: `value` and `page` point into the same live page.
                .map(|value| unsafe { Mut::in_page(value, page, key, observed, epoch, row, tick) }),
        )
    }
}

/// Returns the page `page_id` of `world`, through the page buffer's pointer.
///
/// Never `&mut world.storage.pages[page_id]`: a pointer cast from a reference
/// to one page is invalidated by the next reference made to it, and the
/// guards an earlier fetch handed out note their writes through theirs.
///
/// # Safety
///
/// `world` keeps write permission over the world, and no reference to the
/// page is live.
unsafe fn page_of_world(world: *const World, page_id: u32) -> Option<*mut ComponentPage> {
    // SAFETY: the caller keeps write permission over the world.
    let world = unsafe { &mut *(world as *mut World) };
    let pages = &mut world.storage.pages;
    // SAFETY: `page_id` is in bounds, checked here.
    ((page_id as usize) < pages.len()).then(|| unsafe { pages.as_mut_ptr().add(page_id as usize) })
}

/// Returns the pointer to item `row` of `vec`, through the buffer's pointer.
///
/// Never `vec.get_mut(row)`: that reborrows the whole column as `&mut [T]`,
/// which invalidates every item an earlier fetch handed out from the same
/// column — items a query's caller may still hold.
///
/// # Safety
///
/// No other reference to item `row` may be live.
unsafe fn column_item<T>(vec: &mut Vec<T>, row: usize) -> Option<*mut T> {
    // `row` is in bounds, checked here, and the caller guarantees no other
    // reference to that item is live; the pointer reaches that item alone.
    // SAFETY: `row` is in bounds, checked here.
    (row < vec.len()).then(|| unsafe { vec.as_mut_ptr().add(row) })
}

// A tuple of terms, up to twelve.
macro_rules! impl_query_tuple {
    ($($Q:ident),*) => {
        impl<$($Q: WorldQuery),*> WorldQuery for ($($Q,)*) {
            type Item<'a> = ($($Q::Item<'a>,)*);

            fn type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::type_ids());)*
                ids.sort();
                ids.dedup();
                ids
            }

            fn without_type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::without_type_ids());)*
                ids.sort();
                ids.dedup();
                ids
            }

            fn optional_type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::optional_type_ids());)*
                ids.sort();
                ids.dedup();
                ids
            }

            fn mutable_type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::mutable_type_ids());)*
                ids.sort();
                ids.dedup();
                ids
            }

            fn accessed_type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::accessed_type_ids());)*
                ids
            }

            fn page_passes(page: &ComponentPage, since: Tick) -> bool {
                $($Q::page_passes(page, since))&&*
            }

            fn row_passes(page: &ComponentPage, row: usize, since: Tick) -> bool {
                $($Q::row_passes(page, row, since))&&*
            }

            unsafe fn fetch<'a>(
                page_ptr: *const ComponentPage,
                row_index: usize,
                ctx: FetchContext<'_>,
            ) -> Self::Item<'a> {
                // SAFETY: each term's contract is the tuple's, which the caller upholds.
                ($(unsafe { $Q::fetch(page_ptr, row_index, ctx) },)*)
            }

            unsafe fn fetch_from_world<'a>(
                world: *const World,
                entity_id: EntityId,
                ctx: FetchContext<'_>,
            ) -> Option<Self::Item<'a>> {
                // SAFETY: each term's contract is the tuple's, which the caller upholds.
                Some(($(unsafe { $Q::fetch_from_world(world, entity_id, ctx) }?,)*))
            }
        }
    };
}

impl_query_tuple!(Q1);

impl_query_tuple!(Q1, Q2);

impl_query_tuple!(Q1, Q2, Q3);

impl_query_tuple!(Q1, Q2, Q3, Q4);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q11);

impl_query_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q11, Q12);

/// A query that only reads: what a `&World` may hand out.
///
/// [`World::query`](crate::ecs::World::query) takes `&self`, so it must never
/// yield a `&mut` into the world — two such queries could alias it, and a
/// `&mut T` made from a shared borrow is undefined behavior whatever the code
/// around it does. Writing goes through `query_mut`, which takes `&mut self`.
///
/// # Safety
///
/// Implement only for a query whose items give read access alone, and whose
/// `fetch` and `fetch_from_world` never write through the pointer they get.
///
/// ```
/// # use khora_data::ecs::{Transform, World};
/// let mut world = World::new();
/// // Reading through a shared borrow, writing through an exclusive one.
/// for _transform in world.query::<&Transform>() {}
/// for mut transform in world.query_mut::<&mut Transform>() {
///     *transform = Transform::default();
/// }
/// ```
///
/// ```compile_fail
/// # use khora_data::ecs::{Transform, World};
/// let world = World::new();
/// // A `&mut` term through a shared borrow: refused at compile time.
/// for transform in world.query::<&mut Transform>() {
///     *transform = Transform::default();
/// }
/// ```
pub unsafe trait ReadOnlyWorldQuery: WorldQuery {}

// SAFETY: `&T` reads one component and hands out a shared reference.
unsafe impl<T: Component> ReadOnlyWorldQuery for &T {}
// SAFETY: the same, or nothing.
unsafe impl<T: Component> ReadOnlyWorldQuery for Option<&T> {}
// SAFETY: an id is copied out of the page's entity list.
unsafe impl ReadOnlyWorldQuery for EntityId {}
// SAFETY: a filter yields nothing and only tests the entity's metadata.
unsafe impl<T: Component> ReadOnlyWorldQuery for without::Without<T> {}
// SAFETY: a filter yields nothing and only reads the ticks of `T`'s column.
unsafe impl<T: Component> ReadOnlyWorldQuery for changed::Changed<T> {}
// SAFETY: a field-SoA read clones the component out of its columns.
unsafe impl<T: crate::ecs::SoaLayout> ReadOnlyWorldQuery for columns::Soa<T> {}

macro_rules! impl_read_only_tuple {
    ($($Q:ident),*) => {
        // SAFETY: a tuple reads only if every term reads only.
        unsafe impl<$($Q: ReadOnlyWorldQuery),*> ReadOnlyWorldQuery for ($($Q,)*) {}
    };
}

impl_read_only_tuple!(Q1);
impl_read_only_tuple!(Q1, Q2);
impl_read_only_tuple!(Q1, Q2, Q3);
impl_read_only_tuple!(Q1, Q2, Q3, Q4);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q11);
impl_read_only_tuple!(Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q11, Q12);

// The entity id, read from the page's own entity list.
impl WorldQuery for EntityId {
    type Item<'a> = EntityId;

    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    unsafe fn fetch<'a>(
        page_ptr: *const ComponentPage,
        row_index: usize,
        _ctx: FetchContext<'_>,
    ) -> Self::Item<'a> {
        // SAFETY: the caller guarantees `page_ptr` points to a valid page.
        let page = unsafe { &*page_ptr };
        // SAFETY: the caller guarantees `row_index` is in bounds.
        unsafe { *page.entities.get_unchecked(row_index) }
    }

    unsafe fn fetch_from_world<'a>(
        _world: *const World,
        entity_id: EntityId,
        _ctx: FetchContext<'_>,
    ) -> Option<Self::Item<'a>> {
        Some(entity_id)
    }
}
