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

//! Defines the `ComponentRegistry` and `SemanticDomain` for the CRPECS.

use crate::ecs::Component;
use std::{any::TypeId, collections::HashMap};

/// Defines the semantic domains a component can belong to.
///
/// This is used by the [`ComponentRegistry`] to map a component type to its
/// corresponding `ComponentPage` group. This grouping is the core principle that
/// allows the CRPECS to have fast, domain-specific queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticDomain {
    /// For components related to position, physics, and the scene graph.
    Spatial,
    /// For components related to rendering, such as mesh and material handles.
    Render,
}

/// A registry that maps component types to their semantic domains.
///
/// This is a critical internal part of the `World`. It provides a single source
/// of truth for determining which semantic group a component's data belongs to,
/// enabling the `World` to correctly store and retrieve component data from pages.
#[derive(Debug, Default)]
pub struct ComponentRegistry {
    /// The core map from a component's `TypeId` to its assigned `SemanticDomain`.
    mapping: HashMap<TypeId, SemanticDomain>,
}

impl ComponentRegistry {
    /// (Internal) Registers a component type with a specific semantic domain.
    ///
    /// This should be called by the engine or `World` setup logic for all known component types.
    pub(crate) fn register<T: Component>(&mut self, domain: SemanticDomain) {
        self.mapping.insert(TypeId::of::<T>(), domain);
    }

    /// (Internal) Looks up the `SemanticDomain` for a given component type.
    pub fn domain_of<T: Component>(&self) -> Option<SemanticDomain> {
        self.mapping.get(&TypeId::of::<T>()).copied()
    }
}
