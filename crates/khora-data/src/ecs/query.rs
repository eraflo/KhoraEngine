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
    page::{AnyVec, ComponentPage},
    Component, DomainBitset, QueryMode, QueryPlan, World,
};
use std::{any::TypeId, marker::PhantomData};

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

    /// Returns the sorted list of `TypeId`s for components to be EXCLUDED from the query.
    /// Used to filter out pages that contain these components.
    fn without_type_ids() -> Vec<TypeId> {
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
        let column: &dyn AnyVec = &**page.columns.get(&TypeId::of::<T>()).unwrap();

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

        // Get the entity's metadata.
        let metadata = world.entities.get(entity_id.index as usize)?.1.as_ref()?;

        // Get the domain for the component type.
        let domain = world.registry.get_domain(TypeId::of::<T>())?;

        // Get the location of the entity in the domain.
        let location = metadata.locations.get(&domain)?;

        // Get the page for the entity.
        let page = &world.pages[location.page_id as usize];
        let column = page.columns.get(&TypeId::of::<T>())?;
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
        let column = page.columns.get_mut(&TypeId::of::<T>()).unwrap();
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>().unwrap();
        vec.get_unchecked_mut(row_index)
    }

    unsafe fn fetch_from_world<'a>(
        world: *const World,
        entity_id: EntityId,
    ) -> Option<Self::Item<'a>> {
        let world_mut = &mut *(world as *mut World);
        let metadata = world_mut
            .entities
            .get_mut(entity_id.index as usize)?
            .1
            .as_mut()?;
        let _domain = world_mut.registry.get_domain(TypeId::of::<T>())?;
        let location = metadata.locations.get(&_domain)?;

        let page = &mut world_mut.pages[location.page_id as usize];
        let column = page.columns.get_mut(&TypeId::of::<T>())?;
        let vec = column.as_any_mut().downcast_mut::<Vec<T>>()?;
        vec.get_mut(location.row_index as usize)
    }
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

// -------------------- //
// ---- Query Part ---- //
// -------------------- //

/// An iterator that yields the results of a `WorldQuery`.
///
/// This struct is created by the [`World::query()`] method. It holds a reference
/// to the world and iterates through all `ComponentPage`s that match the query's
/// signature, fetching the requested data for each entity in those pages.
pub struct Query<'a, Q: WorldQuery> {
    /// A raw pointer to the world. Using a pointer avoids lifetime variance issues
    /// and gives us the flexibility to provide either mutable or immutable access.
    world_ptr: *const World,

    /// The list of page indices that match this query (used in Native mode).
    matching_page_indices: Vec<u32>,

    /// The execution plan for this query.
    plan: QueryPlan,

    /// The index of the current page we are iterating through.
    current_page_index: usize,

    /// The index of the next row to fetch within the current page.
    current_row_index: usize,

    /// A marker to associate this iterator with the lifetime `'a` and the query type `Q`.
    /// It tells Rust that our iterator behaves as if it's borrowing `&'a Q` from the world.
    _phantom: PhantomData<(&'a (), Q)>,

    /// Pre-computed bitset intersection for fast-failing transversal lookups.
    combined_bitset: Option<DomainBitset>,
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// (Internal) Creates a new `Query` iterator.
    ///
    /// This is intended to be called only by `World::query()`.
    /// It takes the world, the plan (strategy), and the pre-calculated list
    /// of matching pages as arguments.
    pub(crate) fn new(world: &'a World, plan: QueryPlan, matching_page_indices: Vec<u32>) -> Self {
        let combined_bitset = world.compute_query_bitset(&plan);
        Self {
            world_ptr: world as *const _,
            matching_page_indices,
            plan,
            current_page_index: 0,
            current_row_index: 0,
            _phantom: PhantomData,
            combined_bitset,
        }
    }
}

impl<'a, Q: WorldQuery> Iterator for Query<'a, Q> {
    /// The type of item yielded by this iterator, as defined by the `WorldQuery` trait.
    type Item = Q::Item<'a>;

    /// Advances the iterator and returns the next item.
    fn next(&mut self) -> Option<Self::Item> {
        // Delegate to the appropriate execution path based on the pre-calculated plan.
        match self.plan.mode {
            QueryMode::Native => self.next_native(),
            QueryMode::Transversal => self.next_transversal(),
        }
    }
}

impl<'a, Q: WorldQuery> Query<'a, Q> {
    /// (Internal) Performs a "Native" iteration, fetching data from a single domain.
    /// This is the most efficient execution path.
    fn next_native(&mut self) -> Option<Q::Item<'a>> {
        loop {
            // 1. Check if there are any pages left to iterate through.
            if self.current_page_index >= self.matching_page_indices.len() {
                return None; // No more pages, iteration is finished.
            }

            // Unsafe block because we are dereferencing a raw pointer.
            // This is safe because the `Query` is only created from a valid `World` reference.
            let world = unsafe { &*self.world_ptr };

            // 2. Get the current page.
            let page_id = self.matching_page_indices[self.current_page_index];
            let page = &world.pages[page_id as usize];

            // 3. Check if there are rows left in the current page.
            if self.current_row_index < page.row_count() {
                let item = unsafe {
                    // Safe because the page signature matches the query requirements.
                    Q::fetch(page as *const _, self.current_row_index)
                };

                // Advance the row index for the next call.
                self.current_row_index += 1;
                return Some(item);
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0; // Reset the row index for the new page.
                                            // The `loop` will then re-evaluate with the new page index.
            }
        }
    }

    /// (Internal) Performs a "Transversal" iteration, joining data across domains.
    /// This uses the driver domain to find entities and then pulls peer data from other domains.
    fn next_transversal(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            let world = unsafe { &*self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index];
            let page = &world.pages[page_id as usize];

            if self.current_row_index < page.row_count() {
                // In transversal mode, we use the EntityId from the driver page to look up
                // counterpart components in the peer domains.
                let entity_id = page.entities[self.current_row_index];
                self.current_row_index += 1;

                // Optimization: Skip metadata lookup if the entity is not in the combined bitset.
                if let Some(bitset) = &self.combined_bitset {
                    if !bitset.is_set(entity_id.index) {
                        continue;
                    }
                }

                // Attempt to fetch the Full item (driver + peers) from the world.
                if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, entity_id) } {
                    return Some(item);
                }
                // If the join failed for this entity, we continue to the next one.
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }
}

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
        let world = unsafe { &*world };
        let metadata = world.entities.get(entity_id.index as usize)?.1.as_ref()?;

        // Check ALL pages associated with this entity.
        // If ANY page contains the forbidden component, filtering failed.
        for location in metadata.locations.values() {
            let page = &world.pages[location.page_id as usize];
            if page.type_ids.binary_search(&TypeId::of::<T>()).is_ok() {
                return None;
            }
        }
        Some(())
    }
}

// ------------------------- //
// ---- QueryMut Part ---- //
// ------------------------- //

/// An iterator that yields the results of a mutable `WorldQuery`.
///
/// This struct is created by the [`World::query_mut()`] method.
pub struct QueryMut<'a, Q: WorldQuery> {
    world_ptr: *mut World,
    matching_page_indices: Vec<u32>,
    plan: QueryPlan,
    current_page_index: usize,
    current_row_index: usize,
    _phantom: PhantomData<(&'a (), Q)>,
    /// Pre-computed bitset intersection for fast-failing transversal lookups.
    combined_bitset: Option<DomainBitset>,
}

impl<'a, Q: WorldQuery> QueryMut<'a, Q> {
    /// Creates a new `QueryMut` iterator.
    ///
    /// This is intended to be called only by `World::query_mut()`.
    pub(crate) fn new(
        world: &'a mut World,
        plan: QueryPlan,
        matching_page_indices: Vec<u32>,
    ) -> Self {
        let combined_bitset = world.compute_query_bitset(&plan);
        Self {
            world_ptr: world as *mut _,
            matching_page_indices,
            plan,
            current_page_index: 0,
            current_row_index: 0,
            _phantom: PhantomData,
            combined_bitset,
        }
    }
}

impl<'a, Q: WorldQuery> Iterator for QueryMut<'a, Q> {
    type Item = Q::Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.plan.mode {
            QueryMode::Native => self.next_native(),
            QueryMode::Transversal => self.next_transversal(),
        }
    }
}

impl<'a, Q: WorldQuery> QueryMut<'a, Q> {
    fn next_native(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // SAFETY: `world_ptr` is guaranteed to be valid for the lifetime 'a.
            let world = unsafe { &mut *self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index] as usize;

            // We get a mutable reference to the page, which is a safe operation
            // because `world` is a mutable reference.
            let page = &mut world.pages[page_id];

            if self.current_row_index < page.row_count() {
                let item = unsafe { Q::fetch(page as *mut _ as *const _, self.current_row_index) };

                self.current_row_index += 1;
                return Some(item);
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }

    fn next_transversal(&mut self) -> Option<Q::Item<'a>> {
        loop {
            if self.current_page_index >= self.matching_page_indices.len() {
                return None;
            }

            // Get the current page.
            let world = unsafe { &mut *self.world_ptr };
            let page_id = self.matching_page_indices[self.current_page_index] as usize;
            let page = &mut world.pages[page_id];

            // Check if there are rows left in the current page.
            if self.current_row_index < page.row_count() {
                let entity_id = page.entities[self.current_row_index];
                self.current_row_index += 1;

                // Skip entities that are not in the combined bitset.
                if let Some(combined) = &self.combined_bitset {
                    if !combined.is_set(entity_id.index) {
                        continue;
                    }
                }

                // Attempt to fetch the Full item (driver + peers) from the world.
                if let Some(item) = unsafe { Q::fetch_from_world(world as *const _, entity_id) } {
                    return Some(item);
                }
            } else {
                self.current_page_index += 1;
                self.current_row_index = 0;
            }
        }
    }
}
