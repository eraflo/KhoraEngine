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

use std::{any::TypeId, collections::HashMap};

use crate::ecs::Component;

/// Defines the semantic domains a component can belong to.
///
/// This is used by the `ComponentRegistry` to map a component type to the
/// correct metadata field (e.g., `physics_location`) and to group components
/// into appropriate `ComponentPage`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticDomain {
    /// For components related to position, physics, and scene hierarchy.
    Spatial,
    /// For components related to rendering.
    Render,
    // Add other domains like `Ai`, `Audio`, `Ui` here in the future.
}

/// A registry that maps component types to their semantic domains.
///
/// This is a critical part of the ECS's internal architecture. It provides a single
/// source of truth for where a component's data should be stored and how to
/// find it.
#[derive(Debug, Default)]
pub struct ComponentRegistry {
    /// The core map from a component's `TypeId` to its assigned `SemanticDomain`.
    mapping: HashMap<TypeId, SemanticDomain>,
}

impl ComponentRegistry {
    /// Registers a component type with a specific semantic domain.
    ///
    /// This should be called by the engine setup logic for all known component types.
    pub(crate) fn register<T: Component>(&mut self, domain: SemanticDomain) {
        self.mapping.insert(TypeId::of::<T>(), domain);
    }

    /// Looks up the domain for a given component type.
    pub fn domain_of<T: Component>(&self) -> Option<SemanticDomain> {
        self.mapping.get(&TypeId::of::<T>()).copied()
    }
}
