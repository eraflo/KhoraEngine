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

//! Defines a stable, intermediate representation of a scene using serializable data types.

use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

use crate::ecs::{SerializableParent, SerializableTransform};

/// The root container for a scene's intermediate representation.
#[derive(Debug, Serialize, Deserialize)]
pub struct SceneDefinition {
    /// All entities in the scene.
    pub entities: Vec<EntityDefinition>,
}

/// A serializable representation of a single entity and its components.
#[derive(Debug, Serialize, Deserialize)]
pub struct EntityDefinition {
    /// The unique identifier for the entity.
    pub id: EntityId,
    /// The components attached to the entity.
    pub components: Vec<ComponentDefinition>,
}

/// A serializable, type-erased representation of a single component.
#[derive(Debug, Serialize, Deserialize)]
pub enum ComponentDefinition {
    /// The transform component, representing the entity's position, rotation, and scale.
    Transform(SerializableTransform),
    /// The parent component, representing the entity's parent-child relationship.
    Parent(SerializableParent),
}
