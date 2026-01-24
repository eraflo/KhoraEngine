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

use bincode::{Decode, Encode};

use crate::ecs::{AnyVec, Component};
use std::{
    any::{self, TypeId},
    collections::HashMap,
};

/// Type alias for the row copy function pointer.
type RowCopyFn = unsafe fn(&dyn AnyVec, usize, &mut dyn AnyVec);

/// Defines the semantic domains a component can belong to.
///
/// This is used by the [`ComponentRegistry`] to map a component type to its
/// corresponding `ComponentPage` group. This grouping is the core principle that
/// allows the CRPECS to have fast, domain-specific queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum SemanticDomain {
    /// For components related to position, physics, and the scene graph.
    Spatial,
    /// For components related to rendering, such as mesh and material handles.
    Render,
    /// For components related to audio, such as audio sources and listeners.
    Audio,
}

/// Stores the set of type-erased functions for a registered component.
#[derive(Debug)]
struct ComponentVTable {
    /// The semantic domain this component belongs to.
    domain: SemanticDomain,
    /// Creates a new, empty `Box<dyn AnyVec>` for this component type.
    create_column: fn() -> Box<dyn AnyVec>,
    /// Copies a single element from a source column to a destination column.
    copy_row: RowCopyFn,
}

/// A registry that maps component types to their semantic domains.
///
/// This is a critical internal part of the `World`. It provides a single source
/// of truth for determining which semantic group a component's data belongs to,
/// enabling the `World` to correctly store and retrieve component data from pages.
#[derive(Debug, Default)]
pub struct ComponentRegistry {
    /// Maps a component's `TypeId` to its VTable of operations.
    mapping: HashMap<TypeId, ComponentVTable>,
}

impl ComponentRegistry {
    /// Registers a component type with its domain and lifecycle functions.
    pub(crate) fn register<T: Component>(&mut self, domain: SemanticDomain) {
        self.mapping.insert(
            TypeId::of::<T>(),
            ComponentVTable {
                domain,
                create_column: || Box::new(Vec::<T>::new()),
                copy_row: |src_col, src_row, dest_col| unsafe {
                    let src_vec = src_col.as_any().downcast_ref::<Vec<T>>().unwrap();
                    let dest_vec = dest_col.as_any_mut().downcast_mut::<Vec<T>>().unwrap();
                    dest_vec.push(src_vec.get_unchecked(src_row).clone());
                },
            },
        );
    }

    /// Looks up the `SemanticDomain` for a given `TypeId`.
    pub fn get_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.mapping.get(&type_id).map(|vtable| vtable.domain)
    }

    /// (Internal) Gets the column constructor function for a given TypeId.
    pub(crate) fn get_column_constructor(
        &self,
        type_id: &TypeId,
    ) -> Option<fn() -> Box<dyn AnyVec>> {
        self.mapping.get(type_id).map(|vtable| vtable.create_column)
    }

    /// (Internal) Gets the row copy function for a given TypeId.
    pub(crate) fn get_row_copier(&self, type_id: &TypeId) -> Option<RowCopyFn> {
        self.mapping.get(type_id).map(|vtable| vtable.copy_row)
    }
}

/// A registry that provides reflection data, like type names.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    /// Maps a component's `TypeId` to its string name.
    id_to_name: HashMap<TypeId, String>,
    /// Maps a component's string name to its `TypeId`.
    name_to_id: HashMap<String, TypeId>,
}

impl TypeRegistry {
    /// Registers a component type, storing its name and TypeId.
    pub(crate) fn register<T: Component>(&mut self) {
        let type_id = TypeId::of::<T>();
        let type_name = any::type_name::<T>().to_string();
        self.id_to_name.insert(type_id, type_name.clone());
        self.name_to_id.insert(type_name, type_id);
    }

    /// Gets the string name for a given TypeId.
    pub(crate) fn get_name_of(&self, type_id: &TypeId) -> Option<&str> {
        self.id_to_name.get(type_id).map(|s| s.as_str())
    }

    /// Gets the TypeId for a given string name.
    pub(crate) fn get_id_of(&self, type_name: &str) -> Option<TypeId> {
        self.name_to_id.get(type_name).copied()
    }
}
