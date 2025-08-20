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

use crate::ecs::{ComponentPage, EntityId, EntityMetadata};

/// The central container for the entire ECS, holding all entities, components, and metadata.
///
/// The `World` orchestrates the CRPECS architecture. It owns the data and provides the main
/// API for interacting with the ECS state.
#[derive(Default)]
#[allow(dead_code)]
pub struct World {
    /// A dense list of metadata for every entity that has ever been created.
    /// The index into this vector is used as the `index` part of an `EntityId`.
    /// The `Option` allows us to mark entries as "vacant" when an entity is despawned,
    /// making them available for recycling.
    entities: Vec<Option<(EntityId, EntityMetadata)>>,

    /// A list of all allocated `ComponentPage`s.
    /// A `page_id` in a `PageIndex` corresponds to an index in this vector.
    pages: Vec<ComponentPage>,
    // We will add more fields here later, such as a resource manager
    // or an entity ID allocator to manage recycled generations.
}
