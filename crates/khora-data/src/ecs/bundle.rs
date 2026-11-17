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

use crate::ecs::component::Component;
use crate::ecs::component_registry::ComponentRegistry;
use crate::ecs::entity::EntityMetadata;
use crate::ecs::page::{ComponentPage, PageIndex};
use crate::ecs::{ColumnMap, ComponentKey};

/// A set of components spawned together as one entity: a single component or a tuple.
pub trait ComponentBundle {
    /// Returns the bundle's signature: its component `TypeId`s, sorted and deduplicated.
    ///
    /// Sorted, so `(A, B)` and `(B, A)` find the same page.
    fn type_ids() -> Vec<TypeId>;

    /// Creates one empty column per component, each in the component's own layout.
    fn create_columns() -> ColumnMap;

    /// Returns the name of the first component type `registry` does not know.
    fn unregistered(registry: &ComponentRegistry) -> Option<&'static str>;

    /// Returns the name of a component type this bundle names more than once.
    ///
    /// The signature holds the type once, so a second value would push one row
    /// too many into its column and shift every entity after it.
    fn repeated() -> Option<&'static str> {
        None
    }

    /// Points `metadata` at `location` for the domain of each of the bundle's components.
    fn update_metadata(
        metadata: &mut EntityMetadata,
        location: PageIndex,
        registry: &ComponentRegistry,
    );

    /// Pushes one row of this bundle's components into `page`.
    ///
    /// # Safety
    /// `page` must hold exactly this bundle's columns: each is downcast to its
    /// component's column type.
    unsafe fn add_to_page(self, page: &mut ComponentPage);
}

/// The empty bundle: spawns an entity with no component.
impl ComponentBundle for () {
    fn type_ids() -> Vec<TypeId> {
        Vec::new()
    }

    fn create_columns() -> ColumnMap {
        ColumnMap::default()
    }

    fn unregistered(_registry: &ComponentRegistry) -> Option<&'static str> {
        None
    }

    fn update_metadata(
        _metadata: &mut EntityMetadata,
        _location: PageIndex,
        _registry: &ComponentRegistry,
    ) {
    }

    unsafe fn add_to_page(self, _page: &mut ComponentPage) {}
}

// Implementation for a single component.
impl<C1: Component> ComponentBundle for C1 {
    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<C1>()]
    }

    fn create_columns() -> ColumnMap {
        let mut columns = ColumnMap::default();
        // The column type is the component's choice: AoS, or field-SoA when opted in.
        columns.insert(ComponentKey::of::<C1>(), C1::make_column());
        columns
    }

    fn unregistered(registry: &ComponentRegistry) -> Option<&'static str> {
        registry
            .get_domain(TypeId::of::<C1>())
            .is_none()
            .then(std::any::type_name::<C1>)
    }

    fn update_metadata(
        metadata: &mut EntityMetadata,
        location: PageIndex,
        registry: &ComponentRegistry,
    ) {
        if let Some(domain) = registry.get_domain(TypeId::of::<Self>()) {
            metadata.locations.insert(domain, location);
        }
    }

    unsafe fn add_to_page(self, page: &mut ComponentPage) {
        let column = page
            .columns
            .get_mut(&ComponentKey::of::<C1>())
            .expect("the caller's page holds this bundle's columns")
            .as_mut();
        self.push_into_column(column);
    }
}

// Tuples of 2 to 12 components.
macro_rules! impl_bundle_tuple {
    ($(($C:ident, $idx:tt)),*) => {
        impl<$($C: Component),*> ComponentBundle for ($($C,)*) {
            fn type_ids() -> Vec<TypeId> {
                let mut ids = vec![$(TypeId::of::<$C>()),*];
                ids.sort();
                ids.dedup();
                ids
            }

            fn create_columns() -> ColumnMap {
                let mut columns = ColumnMap::default();
                $(
                    columns.insert(ComponentKey::of::<$C>(), $C::make_column());
                )*
                columns
            }

            fn unregistered(registry: &ComponentRegistry) -> Option<&'static str> {
                $(
                    if registry.get_domain(TypeId::of::<$C>()).is_none() {
                        return Some(std::any::type_name::<$C>());
                    }
                )*
                None
            }

            fn repeated() -> Option<&'static str> {
                let named = [$((TypeId::of::<$C>(), std::any::type_name::<$C>())),*];
                named
                    .iter()
                    .enumerate()
                    .find(|(at, (id, _))| named[..*at].iter().any(|(earlier, _)| earlier == id))
                    .map(|(_, (_, name))| *name)
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
                    let column = page
                        .columns
                        .get_mut(&ComponentKey::of::<$C>())
                        .expect("the caller's page holds this bundle's columns")
                        .as_mut();
                    self.$idx.push_into_column(column);
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
