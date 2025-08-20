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
    fn update_metadata(metadata: &mut EntityMetadata, location: PageIndex);

    /// Adds the components from this bundle into the specified `ComponentPage`.
    ///
    /// # Safety
    /// This function is unsafe because it relies on the caller to guarantee that
    /// the `ComponentPage` is the correct one for this bundle's exact component layout.
    /// It performs unsafe downcasting to write to the type-erased `Vec<T>`s.
    unsafe fn add_to_page(self, page: &mut ComponentPage);
}

// Implementation for a 2-component tuple
impl<C1: Component, C2: Component> ComponentBundle for (C1, C2) {
    fn type_ids() -> Vec<TypeId> {
        let mut ids = vec![TypeId::of::<C1>(), TypeId::of::<C2>()];
        // It's crucial to sort the IDs to have a canonical signature.
        ids.sort();
        ids
    }

    fn create_columns() -> HashMap<TypeId, Box<dyn AnyVec>> {
        // Create a HashMap to hold the type-erased component vectors.
        let mut columns: HashMap<TypeId, Box<dyn AnyVec>> = HashMap::new();

        // For each component in the bundle, create an empty `Vec<T>`,
        // box it, and insert it into the map with its `TypeId` as the key.
        columns.insert(
            TypeId::of::<C1>(),
            Box::new(Vec::<C1>::new()) as Box<dyn AnyVec>,
        );
        columns.insert(
            TypeId::of::<C2>(),
            Box::new(Vec::<C2>::new()) as Box<dyn AnyVec>,
        );

        columns
    }

    fn update_metadata(metadata: &mut EntityMetadata, location: PageIndex) {
        // --- CURRENT LIMITATION ---
        // For now, we make a simplistic assumption: any bundle we create belongs to the "physics" domain.
        // This is a temporary simplification to allow us to move forward. Later, we will need
        // a more sophisticated system to associate components with domains.
        metadata.physics_location = Some(location);
    }

    unsafe fn add_to_page(self, page: &mut ComponentPage) {
        // --- THIS IS A CRITICAL SAFETY REGION ---

        // 1. Store the TypeIds in local variables to create references with a valid lifetime.
        let type_id1 = TypeId::of::<C1>();
        let type_id2 = TypeId::of::<C2>();

        // 2. Assert that we are not trying to mutably alias the same component type.
        // This is a critical runtime safety check for our logic.
        assert_ne!(
            type_id1, type_id2,
            "Bundles cannot contain duplicate component types."
        );

        // 3. Get mutable references to both columns simultaneously using the correct, stable API.
        let [c1_anyvec, c2_anyvec] = page.columns.get_disjoint_mut([&type_id1, &type_id2]);

        // 4. Safely unwrap and downcast each reference.
        // We can safely unwrap here because the `unsafe` contract of this function
        // guarantees that the columns exist.
        let c1_vec = c1_anyvec
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Vec<C1>>()
            .unwrap();
        let c2_vec = c2_anyvec
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Vec<C2>>()
            .unwrap();

        // 5. Push the component data.
        c1_vec.push(self.0);
        c2_vec.push(self.1);
    }
}
