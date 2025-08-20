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

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use crate::ecs::EntityId;

/// An internal helper trait to perform vector operations on a type-erased `Box<dyn Any>`.
///
/// This allows us to call methods like `swap_remove` on component columns without
/// needing to know their concrete `Vec<T>` type at compile time.
pub trait AnyVec {
    /// Casts the trait object to `&dyn Any`.
    fn as_any(&self) -> &dyn Any;

    /// Casts the trait object to `&mut dyn Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Performs a `swap_remove` on the underlying `Vec`, removing the element at `index`.
    fn swap_remove_any(&mut self, index: usize);
}

// We implement this trait for any `Vec<T>` where T is `'static`.
impl<T: 'static> AnyVec for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn swap_remove_any(&mut self, index: usize) {
        self.swap_remove(index);
    }
}

/// A logical address pointing to an entity's component data within a specific `ComponentPage`.
///
/// This struct is the core of the relational aspect of our ECS. It decouples an entity's
/// identity from the physical storage of its data by acting as a coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageIndex {
    /// The unique identifier of the `ComponentPage` that stores the component data.
    pub page_id: u32,
    /// The index of the row within the page where this entity's components are stored.
    pub row_index: u32,
}

/// A page of memory that stores the component data for multiple entities
/// in a Structure of Arrays (SoA) layout.
///
/// A `ComponentPage` is specialized for a single semantic domain (e.g., physics).
/// It contains multiple columns, where each column is a `Vec<T>` for a specific
/// component type `T`. This SoA layout is the key to our high iteration performance,
/// as it guarantees contiguous data access for native queries.
pub struct ComponentPage {
    /// A map from a component's `TypeId` to its actual storage column.
    /// The `Box<dyn AnyVec>` is a type-erased `Vec<T>` that knows how to
    /// perform basic vector operations like `swap_remove`.
    pub(crate) columns: HashMap<TypeId, Box<dyn AnyVec>>,

    /// A list of the `EntityId`s that own the data in each row of this page.
    /// The entity at `entities[i]` corresponds to the components at `columns[...][i]`.
    /// This is crucial for reverse lookups, especially during entity despawning.
    pub(crate) entities: Vec<EntityId>,

    /// The sorted list of `TypeId`s for the components stored in this page.
    /// This acts as the page's "signature" for matching with bundles. It is
    /// kept sorted to ensure that the signature is canonical.
    pub(crate) type_ids: Vec<TypeId>,
}

impl ComponentPage {
    /// Adds an entity to this page's entity list.
    ///
    /// This method is called by `World::spawn` and is a crucial part of maintaining
    /// the invariant that the number of rows in the component columns is always
    /// equal to the number of entities tracked by the page.
    pub(crate) fn add_entity(&mut self, entity_id: EntityId) {
        self.entities.push(entity_id);
    }

    /// Performs a `swap_remove` on a specific row across all component columns
    /// and the entity list.
    ///
    /// This is the core of an O(1) despawn operation. It removes the data for the entity
    /// at `row_index` by swapping it with the last element in each column and in the
    /// entity list.
    ///
    /// It's the caller's (`World::despawn`) responsibility to update the metadata of
    /// the entity that was moved from the last row.
    pub(crate) fn swap_remove_row(&mut self, row_index: u32) {
        // 1. Remove the corresponding entity ID from the list. `swap_remove` on a Vec
        // returns the element that was at that index, but we don't need it here.
        self.entities.swap_remove(row_index as usize);

        // 2. Iterate through all component columns and perform the same swap_remove
        // on each one, using our `AnyVec` trait.
        for column in self.columns.values_mut() {
            column.swap_remove_any(row_index as usize);
        }
    }

    /// Returns the number of rows of data (and entities) this page currently stores.
    #[allow(dead_code)]
    pub(crate) fn row_count(&self) -> usize {
        self.entities.len()
    }
}
