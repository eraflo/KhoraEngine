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
    Component, ComponentKey, SemanticDomain, World,
};
use std::any::TypeId;

mod columns;
mod read;
mod without;
mod write;

pub use columns::Soa;
pub use read::Query;
pub use without::Without;
pub use write::QueryMut;

/// Returns `true` if `row` in `page_id` is the entity's **live** location for
/// `domain` — i.e. not a stale orphan row left behind by a migration.
///
/// `add_component` / `remove_component` move an entity's domain row to a new
/// page but leave the old row in place (reclaimed later by the maintenance GC),
/// and the old page's signature is unchanged, so `find_matching_pages` keeps
/// matching it. Only the row the entity's metadata points to is real; any other
/// row bearing the same entity is a stale orphan (holding outdated component
/// values) that must not be yielded. A row whose entity is dead is never live:
/// the generation check keeps a recycled index from resolving a dead orphan to
/// the new entity. Both the Native and the Transversal iterators call this per
/// driver-page row.
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

/// Returns `entity`'s metadata if it is alive — its slot's generation matches —
/// or `None` for a dead `EntityId`, even when its index has been recycled.
fn live_metadata(world: &World, entity: EntityId) -> Option<&EntityMetadata> {
    world
        .entities
        .get(entity.index as usize)
        .filter(|(slot, _)| slot.generation == entity.generation)
        .and_then(|(_, meta)| meta.as_ref())
}

/// Returns the live location of `entity`'s row in the domain component `T` is
/// registered in, or `None` if the entity is dead, `T` is unregistered, or the
/// entity has no row in that domain.
fn live_location<T: 'static>(world: &World, entity: EntityId) -> Option<PageIndex> {
    let domain = world.storage.registry.get_domain(TypeId::of::<T>())?;
    live_metadata(world, entity)?
        .locations
        .get(&domain)
        .copied()
}

/// Returns `true` if the live `entity` holds any of `filters`' types in its row
/// of that type's domain — the per-row half of a Native `Without` filter whose
/// type lives outside the driver domain (see [`NativeRowPlan::foreign_without`]),
/// which the page-level exclusion of `find_matching_pages` cannot see.
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

/// The per-row work a Native query needs beyond reading its driver row, decided
/// once at query construction by `World::native_row_plan`.
///
/// A Native query iterates the pages of its driver domain, but a `Without` or
/// `Option` term may name a component registered in another domain, stored in
/// another page than the driver row. The default (empty) plan — every term in
/// the driver domain — keeps the per-row path check-free.
#[derive(Debug, Default)]
pub(crate) struct NativeRowPlan {
    /// `Without` types registered outside the driver domain, with their domain,
    /// checked per row by `has_foreign_excluded`.
    pub(crate) foreign_without: Vec<(SemanticDomain, TypeId)>,
    /// An `Option` term names a type registered outside the driver domain: the
    /// page-row `fetch` would read it from the driver page, which never holds
    /// it, so each live row's item is joined through the entity's live
    /// locations with `fetch_from_world` instead (which also checks every
    /// `Without` term, so `foreign_without` is left empty).
    pub(crate) fetch_from_world: bool,
}

// ------------------------- //
// ---- WorldQuery Part ---- //
// ------------------------- //

/// A trait implemented by types that can be used to query data from the `World`.
///
/// This "sealed" trait provides the necessary information for the query engine to
/// find the correct pages and safely access component data. It is implemented for
/// component references (`&T`, `&mut T`), `EntityId`, filters like `Without<T>`,
/// and tuples of other `WorldQuery` types.
pub trait WorldQuery {
    /// The type of item that the query iterator will yield (e.g., `(&'a Position, &'a mut Velocity)`).
    type Item<'a>;

    /// Returns the sorted list of `TypeId`s for the components to be INCLUDED in the query.
    /// This signature is used to find `ComponentPage`s that contain all these components.
    fn type_ids() -> Vec<TypeId>;

    /// Every component the items reach, once per term that reaches it — not
    /// deduplicated, so a query naming one component twice can be told from
    /// one naming it once.
    fn accessed_type_ids() -> Vec<TypeId> {
        let mut ids = Self::type_ids();
        ids.extend(Self::optional_type_ids());
        ids
    }

    /// Returns the sorted list of `TypeId`s for components to be EXCLUDED from the query.
    /// Used to filter out pages that contain these components.
    fn without_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Returns the `TypeId`s of the components read by `Option` terms. They do
    /// not constrain which pages match, but the query engine must know their
    /// domains to read them from the right page.
    fn optional_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Returns the `TypeId`s of the components this query accesses **mutably**
    /// (`&mut T` / `Option<&mut T>` terms). [`World::query_mut`] uses this to
    /// mark the matching domains changed once per query construction.
    /// Read-only terms and filters return nothing (the default).
    fn mutable_type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    /// Fetches the query's item from a specific row in a `ComponentPage`.
    ///
    /// # Safety
    ///
    /// This function is unsafe because the caller (the `Query` iterator) must guarantee
    /// several invariants:
    /// 1. The page pointed to by `page_ptr` is valid and matches the query's signature.
    /// 2. `row_index` is a valid index within the page's columns.
    /// 3. Aliasing rules are not violated (e.g., no two `&mut T` to the same data).
    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a>;

    /// Fetches the query's item directly from the world for a specific entity.
    ///
    /// # Safety
    /// Caller must ensure `world` is valid and no aliasing rules are violated.
    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>>;
}

// Implementation for a query of a single, immutable component reference.
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
    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        // 1. Get a reference to the `ComponentPage`.
        let page = &*page_ptr;

        // 2. Get the type-erased column for the component `T`.
        // We can unwrap because the caller guarantees the column exists.
        let column: &dyn AnyVec = &**page.columns.get(&ComponentKey::of::<T>()).unwrap();

        // 3. Downcast the column to its concrete `Vec<T>` type.
        // First, cast to `&dyn Any`, then `downcast_ref`.
        let vec: &Vec<T> = column.as_any().downcast_ref::<Vec<T>>().unwrap();

        // 4. Get the component from the vector at the specified row.
        // We use `get_unchecked` for performance, as the caller guarantees the
        // index is in bounds. This avoids a bounds check.
        vec.get_unchecked(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        let world = &*world;

        // Get the entity's live row in `T`'s domain (`None` if it is dead).
        let location = live_location::<T>(world, entity_id)?;

        // Get the page for the entity.
        let page = &world.storage.pages[location.page_id as usize];
        let column = page.columns.get(&ComponentKey::of::<T>())?;
        let vec = column.as_any().downcast_ref::<Vec<T>>()?;
        vec.get(location.row_index as usize)
    }
}

// Implementation for a query of a single, mutable component reference.
impl<T: Component> WorldQuery for &mut T {
    type Item<'a> = &'a mut T;

    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    fn mutable_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    /// Fetches a mutable reference to the component `T` from the specified row.
    ///
    /// # Safety
    /// The caller MUST guarantee that:
    /// 1. The page contains a column for component `T`.
    /// 2. `row_index` is in bounds.
    /// 3. No other mutable reference to this specific component exists at the same time.
    ///    The query engine must enforce this.
    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        // UNSAFE: We cast the const pointer to a mutable one.
        // This is safe ONLY if the query engine guarantees no other access.
        let page = &mut *(page_ptr as *mut ComponentPage);
        let column = page.columns.get_mut(&ComponentKey::of::<T>()).unwrap();
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>().unwrap();
        debug_assert!(row_index < vec.len());
        // Through the buffer's pointer, never a slice: `get_unchecked_mut`
        // reborrows the whole column `&mut`, which invalidates every item an
        // earlier call handed out from it.
        &mut *vec.as_mut_ptr().add(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        let location = live_location::<T>(&*world, entity_id)?;
        let world_mut = &mut *(world as *mut World);

        let page = &mut world_mut.storage.pages[location.page_id as usize];
        let column = page.columns.get_mut(&ComponentKey::of::<T>())?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;
        column_item(vec, location.row_index as usize)
    }
}

// Implementation for an optional immutable component reference.
impl<T: Component> WorldQuery for Option<&T> {
    type Item<'a> = Option<&'a T>;

    fn type_ids() -> Vec<TypeId> {
        // Optional components do NOT drive the query signature.
        Vec::new()
    }

    fn optional_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        let page = &*page_ptr;
        let column = page.columns.get(&ComponentKey::of::<T>())?;
        let vec = column.as_any().downcast_ref::<Vec<T>>()?;
        vec.get(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        let world = &*world;
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

// Implementation for an optional mutable component reference.
impl<T: Component> WorldQuery for Option<&mut T> {
    type Item<'a> = Option<&'a mut T>;

    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn optional_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    fn mutable_type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }

    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        let page = &mut *(page_ptr as *mut ComponentPage);
        let column = page.columns.get_mut(&ComponentKey::of::<T>())?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;
        column_item(vec, row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        // Same contract as `Option<&T>`: a dead entity fails the join, a live
        // one lacking `T` yields `Some(None)`.
        live_metadata(&*world, entity_id)?;
        let Some(location) = live_location::<T>(&*world, entity_id) else {
            return Some(None);
        };
        let world_mut = &mut *(world as *mut World);

        let page = &mut world_mut.storage.pages[location.page_id as usize];
        Some(
            page.columns
                .get_mut(&ComponentKey::of::<T>())
                .and_then(|column| column.as_any_mut().downcast_mut::<Vec<T>>())
                .and_then(|vec| column_item(vec, location.row_index as usize)),
        )
    }
}

/// The `&mut` to item `row` of `vec`, through the buffer's pointer.
///
/// Never `vec.get_mut(row)`: that reborrows the whole column as `&mut [T]`,
/// which invalidates every item an earlier fetch handed out from the same
/// column — items a query's caller may still hold.
///
/// # Safety
///
/// No other reference to item `row` may be live.
unsafe fn column_item<'a, T>(vec: &mut Vec<T>, row: usize) -> Option<&'a mut T> {
    // `row` is in bounds, checked here, and the caller guarantees no other
    // reference to that item is live; the pointer reaches that item alone.
    (row < vec.len()).then(|| &mut *vec.as_mut_ptr().add(row))
}

// Implementation for tuples of WorldQuery types.
// We use a macro to avoid "infinity" of manual implementations while maintaining
// the same rigorous safety standards as the single-component cases.
macro_rules! impl_query_tuple {
    ($($Q:ident),*) => {
        impl<$($Q: WorldQuery),*> WorldQuery for ($($Q,)*) {
            type Item<'a> = ($($Q::Item<'a>,)*);

            fn type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::type_ids());)*
                ids.sort();
                ids.dedup(); // Ensure unique TypeIds for canonical signature
                ids
            }

            fn without_type_ids() -> Vec<TypeId> {
                let mut ids = Vec::new();
                $(ids.extend($Q::without_type_ids());)*
                ids.sort();
                ids.dedup(); // Ensure unique TypeIds for canonical signature
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

            unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
                ($($Q::fetch(page_ptr, row_index),)*)
            }

            unsafe fn fetch_from_world<'a>(
                world: *const World,
                entity_id: EntityId,
            ) -> Option<Self::Item<'a>> {
                Some(($($Q::fetch_from_world(world, entity_id)?,)*))
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
/// for transform in world.query_mut::<&mut Transform>() {
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

// To fetch an entity's ID, we need to access the page's own entity list.
// We also need to query for the entity ID itself.
impl WorldQuery for EntityId {
    type Item<'a> = EntityId;

    // EntityId is not a component, it doesn't have a TypeId in the page signature.
    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    // It doesn't have a `without` filter either.
    // The default implementation is sufficient.

    unsafe fn fetch<'a>(page_ptr: *const ComponentPage, row_index: usize) -> Self::Item<'a> {
        let page = &*page_ptr;
        // The entity ID is fetched from the page's own list of entities.
        *page.entities.get_unchecked(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        _world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        // We already have the entity ID, just return it.
        Some(entity_id)
    }
}
