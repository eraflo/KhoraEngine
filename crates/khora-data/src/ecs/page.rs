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

use bincode::{Decode, Encode};
use khora_core::ecs::entity::EntityId;

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

    /// # Safety
    /// Returns the raw byte slice of the underlying `Vec<T>`.
    /// The caller must ensure that this byte representation is handled correctly.
    unsafe fn as_bytes(&self) -> &[u8];

    /// # Safety
    /// Replaces the contents of the `Vec<T>` with the given raw bytes.
    /// The caller must guarantee that the bytes represent a valid sequence of `T`
    /// with the correct size and alignment.
    unsafe fn set_from_bytes(&mut self, bytes: &[u8]);
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

    unsafe fn as_bytes(&self) -> &[u8] {
        std::slice::from_raw_parts(
            self.as_ptr() as *const u8,
            self.len() * std::mem::size_of::<T>(),
        )
    }

    unsafe fn set_from_bytes(&mut self, bytes: &[u8]) {
        let elem_size = std::mem::size_of::<T>();
        if elem_size == 0 {
            return; // Correctly handle Zero-Sized Types.
        }
        assert_eq!(
            bytes.len() % elem_size,
            0,
            "Byte slice length is not a multiple of element size"
        );

        // Calculate the new length and resize the Vec accordingly.
        let new_len = bytes.len() / elem_size;
        self.clear();
        self.reserve(new_len);

        // Perform the copy directly into the Vec's allocated memory.
        let ptr = self.as_mut_ptr() as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        self.set_len(new_len);
    }
}

/// A logical address pointing to an entity's component data within a specific `ComponentPage`.
///
/// This struct is the core of the relational aspect of our ECS. It decouples an entity's
/// identity from the physical storage of its data by acting as a coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct PageIndex {
    /// The unique identifier of the `ComponentPage` that stores the component data.
    pub page_id: u32,
    /// The index of the row within the page where this entity's components are stored.
    pub row_index: u32,
}

/// A serializable representation of a single `ComponentPage`.
#[derive(Encode, Decode)]
pub(crate) struct SerializedPage {
    /// The unique identifiers of this page.
    pub(crate) type_names: Vec<String>,
    /// The list of entities whose component data is stored in this page.
    pub(crate) entities: Vec<EntityId>,
    /// The actual serialized component data columns. Each column is a byte vector
    /// representing the serialized `Vec<T>` for a specific component
    pub(crate) columns: HashMap<String, Vec<u8>>,
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
