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

use std::any::TypeId;
use std::collections::HashMap;

use crate::ecs::component::Component;
use crate::ecs::entity::EntityMetadata;
use crate::ecs::page::{AnyVec, ComponentPage, PageIndex};
use crate::ecs::registry::ComponentRegistry;

/// A trait for any collection of components that can be spawned together as a single unit.
///
/// This is a key part of the ECS's public API. It is typically implemented on tuples
/// of components, like `(Position, Velocity)`. It provides the logic for identifying
/// its component types and safely writing its data into a `ComponentPage`.
pub trait ComponentBundle {
    /// Returns the sorted list of `TypeId`s for the components in this bundle.
    ///
    /// This provides a canonical "signature" for the bundle, which is used to find
    /// a matching `ComponentPage` in the `World`. Sorting is crucial to ensure that
    /// tuples with the same components but in a different order (e.g., (A, B) vs (B, A))
    /// are treated as identical.
    fn type_ids() -> Vec<TypeId>;

    /// Creates the set of empty, type-erased `Vec<T>` columns required to store
    /// this bundle's components.
    ///
    /// This is called by the `World` when a new `ComponentPage` needs to be
    /// created for a specific bundle layout.
    fn create_columns() -> HashMap<TypeId, Box<dyn AnyVec>>;

    /// Updates the appropriate fields in an `EntityMetadata` struct to point
    /// to the location of this bundle's data.
    ///
    /// This method is called by `World::spawn` to link an entity to its newly
    /// created component data.
    fn update_metadata(
        metadata: &mut EntityMetadata,
        location: PageIndex,
        registry: &ComponentRegistry,
    );

    /// Adds the components from this bundle into the specified `ComponentPage`.
    ///
    /// # Safety
    /// This function is unsafe because it relies on the caller to guarantee that
    /// the `ComponentPage` is the correct one for this bundle's exact component layout.
    /// It performs unsafe downcasting to write to the type-erased `Vec<T>`s.
    unsafe fn add_to_page(self, page: &mut ComponentPage);
}

/// Implementation for the empty tuple, allowing spawning of empty entities.
impl ComponentBundle for () {
    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn create_columns() -> HashMap<TypeId, Box<dyn AnyVec>> {
        HashMap::new()
    }

    fn update_metadata(
        _metadata: &mut EntityMetadata,
        _location: PageIndex,
        _registry: &ComponentRegistry,
    ) {
        // No components, so no location information to update.
    }

    unsafe fn add_to_page(self, _page: &mut ComponentPage) {
        // No components to add, so this is a no-op.
    }
}

// Implementation for a single component.
impl<C1: Component> ComponentBundle for C1 {
    fn type_ids() -> Vec<TypeId> {
        // The signature is just the TypeId of the single component.
        vec![TypeId::of::<C1>()]
    }

    fn create_columns() -> HashMap<TypeId, Box<dyn AnyVec>> {
        let mut columns: HashMap<TypeId, Box<dyn AnyVec>> = HashMap::new();
        columns.insert(
            TypeId::of::<C1>(),
            Box::new(Vec::<C1>::new()) as Box<dyn AnyVec>,
        );
        columns
    }

    fn update_metadata(
        metadata: &mut EntityMetadata,
        location: PageIndex,
        registry: &ComponentRegistry,
    ) {
        // Find the domain for this component type in the registry.
        if let Some(domain) = registry.get_domain(TypeId::of::<Self>()) {
            // Insert or update the location for that domain.
            metadata.locations.insert(domain, location);
        }
        // Note: We might want to log a warning here if a component is not registered.
    }

    unsafe fn add_to_page(self, page: &mut ComponentPage) {
        // Get the column and push the single component.
        page.columns
            .get_mut(&TypeId::of::<C1>())
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Vec<C1>>()
            .unwrap()
            .push(self);
    }
}

// Implementation for tuples of components.
// We use a macro to handle various tuple sizes efficienty, following the
// same architectural patterns as `WorldQuery`.
macro_rules! impl_bundle_tuple {
    ($(($C:ident, $idx:tt)),*) => {
        impl<$($C: Component),*> ComponentBundle for ($($C,)*) {
            fn type_ids() -> Vec<TypeId> {
                let mut ids = vec![$(TypeId::of::<$C>()),*];
                ids.sort();
                ids.dedup();
                ids
            }

            fn create_columns() -> HashMap<TypeId, Box<dyn AnyVec>> {
                let mut columns: HashMap<TypeId, Box<dyn AnyVec>> = HashMap::new();
                $(
                    columns.insert(TypeId::of::<$C>(), Box::new(Vec::<$C>::new()) as Box<dyn AnyVec>);
                )*
                columns
            }

            fn update_metadata(
                metadata: &mut EntityMetadata,
                location: PageIndex,
                registry: &ComponentRegistry,
            ) {
                $(
                    if let Some(domain) = registry.get_domain(TypeId::of::<$C>()) {
                        metadata.locations.insert(domain, location);
                    }
                )*
            }

            unsafe fn add_to_page(self, page: &mut ComponentPage) {
                $(
                    page.columns
                        .get_mut(&TypeId::of::<$C>())
                        .unwrap()
                        .as_any_mut()
                        .downcast_mut::<Vec<$C>>()
                        .unwrap()
                        .push(self.$idx);
                )*
            }
        }
    };
}

impl_bundle_tuple!((C1, 0), (C2, 1));
impl_bundle_tuple!((C1, 0), (C2, 1), (C3, 2));
impl_bundle_tuple!((C1, 0), (C2, 1), (C3, 2), (C4, 3));
impl_bundle_tuple!((C1, 0), (C2, 1), (C3, 2), (C4, 3), (C5, 4));
impl_bundle_tuple!((C1, 0), (C2, 1), (C3, 2), (C4, 3), (C5, 4), (C6, 5));
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6)
);
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6),
    (C8, 7)
);
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6),
    (C8, 7),
    (C9, 8)
);
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6),
    (C8, 7),
    (C9, 8),
    (C10, 9)
);
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6),
    (C8, 7),
    (C9, 8),
    (C10, 9),
    (C11, 10)
);
impl_bundle_tuple!(
    (C1, 0),
    (C2, 1),
    (C3, 2),
    (C4, 3),
    (C5, 4),
    (C6, 5),
    (C7, 6),
    (C8, 7),
    (C9, 8),
    (C10, 9),
    (C11, 10),
    (C12, 11)
);
