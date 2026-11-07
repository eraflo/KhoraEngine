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

//! What preparing a load reads of a world, frozen so it can be read off the
//! frame.
//!
//! A load is prepared without the `World`: it reads the domain each
//! component is kept in, the components declared while the engine runs, and
//! the entity a reference to nothing names. The world publishes those as a
//! [`LoadCatalog`], an immutable snapshot shared as an `Arc`. Its
//! `generation` moves whenever what it holds changes, so a load prepared
//! against an older catalogue can be told apart.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{ComponentKey, ComponentRegistry, PackedLayout, SemanticDomain};

/// What a load prepared off the frame reads of a world, as it was when the
/// catalogue was taken.
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

/// A component declared while the engine runs, as the catalogue knows it.
#[derive(Debug, Clone)]
pub struct CatalogDeclared {
    /// Its key in the world's pages.
    pub key: ComponentKey,
    /// The domain the world keeps it in.
    pub domain: SemanticDomain,
    /// Its fields, as the world lays its rows out.
    pub layout: Arc<PackedLayout>,
    /// Whether a game save keeps it to resume.
    pub resumable: bool,
}

impl LoadCatalog {
    /// The catalogue's generation: equal for two catalogues taken from one
    /// world with no change of its components in between, different
    /// otherwise.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The domain the world keeps component `key` in, `None` when the world
    /// does not know it.
    pub fn domain_of(&self, key: ComponentKey) -> Option<SemanticDomain> {
        self.domains.get(&key).copied()
    }

    /// The component declared while the engine runs under `name`, `None`
    /// when the world declares none — a Rust component's name included.
    pub fn declared(&self, name: &str) -> Option<&CatalogDeclared> {
        self.declared.get(name)
    }

    /// Whether the world registers a Rust component under `name`.
    pub fn names_rust(&self, name: &str) -> bool {
        self.rust.contains(name)
    }

    /// The entity a reference to nothing names in the world.
    pub fn nowhere(&self) -> EntityId {
        self.nowhere
    }

    /// The catalogue of `registry`, a world's, whose reference to nothing
    /// names `nowhere`.
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
