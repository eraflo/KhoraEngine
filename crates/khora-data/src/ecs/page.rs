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

use std::{any::{Any, TypeId}, collections::HashMap};

use crate::ecs::EntityId;


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
    /// The `Box<dyn Any>` is a type-erased `Vec<T>`, allowing us to store
    /// different component vectors (e.g., `Vec<Position>`, `Vec<Velocity>`)
    /// in the same collection.
    columns: HashMap<TypeId, Box<dyn Any>>,

    /// A list of the `EntityId`s that own the data in each row of this page.
    /// The entity at `entities[i]` corresponds to the components at `columns[...][i]`.
    /// This is crucial for reverse lookups, especially during entity despawning.
    entities: Vec<EntityId>,
}