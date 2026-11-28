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

//! What preparing a load reads of a world, frozen as a [`LoadCatalog`] so a
//! load can be prepared off the frame, without the `World`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{ComponentKey, ComponentRegistry, SemanticDomain};

mod catalog_declared;

pub use catalog_declared::CatalogDeclared;

/// What a load prepared off the frame reads of a world, as it was when taken.
#[derive(Debug)]
pub struct LoadCatalog {
    generation: u64,
    nowhere: EntityId,
    /// The domain of every component the world registers.
    domains: HashMap<ComponentKey, SemanticDomain>,
    /// The components declared while the engine runs, by name.
    declared: HashMap<String, CatalogDeclared>,
    /// The names of the Rust components the world registers.
    rust: HashSet<String>,
}

impl LoadCatalog {
    /// Returns the catalogue's generation.
    ///
    /// Two catalogues of one world have the same generation exactly when its
    /// components did not change between them.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the domain the world keeps component `key` in, `None` when it does not know it.
    pub fn domain_of(&self, key: ComponentKey) -> Option<SemanticDomain> {
        self.domains.get(&key).copied()
    }

    /// Returns the component declared under `name` while the engine runs.
    ///
    /// `None` for a Rust component's name, or one the world does not declare.
    pub fn declared(&self, name: &str) -> Option<&CatalogDeclared> {
        self.declared.get(name)
    }

    /// Returns the entity a reference to nothing names in the world.
    pub fn nowhere(&self) -> EntityId {
        self.nowhere
    }

    /// Returns the declaration a file holds for the component declared under `name`.
    ///
    /// It equals the one the world's registry builds; `None` when the world
    /// declares no component of that name.
    pub fn declaration_of(&self, name: &str) -> Option<super::declared::DeclaredRecord> {
        let declared = self.declared.get(name)?;
        Some(super::declared::declaration(
            name,
            declared.domain,
            &declared.layout,
            declared.resumable,
        ))
    }

    /// Returns whether the world registers a Rust component under `name`.
    pub(crate) fn names_rust(&self, name: &str) -> bool {
        self.rust.contains(name)
    }

    /// Takes the catalogue of a world's `registry`, whose reference to nothing names `nowhere`.
    pub(crate) fn of(registry: &ComponentRegistry, nowhere: EntityId) -> Self {
        let mut domains = HashMap::with_capacity(registry.len());
        let mut declared = HashMap::new();
        let mut rust = HashSet::new();
        for vtable in registry.iter() {
            domains.insert(vtable.key, vtable.domain);
            if vtable.rust.is_some() {
                rust.insert(vtable.name.to_string());
            } else if let Some(layout) = vtable.columns.packed() {
                declared.insert(
                    vtable.name.to_string(),
                    CatalogDeclared {
                        key: vtable.key,
                        domain: vtable.domain,
                        layout: Arc::new(layout.clone()),
                        resumable: vtable.resumable,
                    },
                );
            }
        }
        Self {
            generation: registry.generation(),
            nowhere,
            domains,
            declared,
            rust,
        }
    }
}

#[cfg(test)]
mod tests;
