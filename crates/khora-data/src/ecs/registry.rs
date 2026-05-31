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
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// For components related to physics simulation.
    Physics,
    /// For components driving the in-world UI subsystem (UiNode, UiText, etc).
    Ui,
}

/// How a component column is physically laid out in memory.
///
/// **AGDF** (adaptive data *layout*) adapts this per component as Data
/// self-maintenance, observed by the DCC. The default is plain `Soa`, so this
/// descriptor is **inert** until an actual repack happens — adding it changes
/// no behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPolicy {
    /// Structure-of-Arrays: one contiguous `Vec<T>` (today's only layout).
    #[default]
    Soa,
    /// Array-of-Structures-of-Arrays: SIMD-friendly tiles of `lanes` elements.
    AoSoA {
        /// Tile width in elements (e.g. 8 for AVX2 `f32`).
        lanes: u8,
    },
}

/// Online access-pattern counters for one component type.
///
/// Updated **once per query** (off the per-element hot path), read by the DCC /
/// telemetry to decide whether a memory-layout repack would pay off. Atomics so
/// they can be recorded through `&self`. One small entry per component *type*
/// (not per entity): the memory cost is constant, not proportional to the world.
#[derive(Debug, Default)]
pub struct AccessCounters {
    /// Number of queries that touched this component.
    pub query_count: AtomicU64,
    /// Cumulative rows visited across those queries.
    pub rows_scanned: AtomicU64,
}

/// Stores the set of type-erased functions for a registered component.
#[derive(Debug)]
struct ComponentVTable {
    /// The semantic domain this component belongs to.
    domain: SemanticDomain,
    /// Current physical layout of this component's columns (default `Soa`).
    layout: LayoutPolicy,
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
    /// Per-component-type online access-pattern counters (layout adaptation).
    access: HashMap<TypeId, AccessCounters>,
}

impl ComponentRegistry {
    /// Registers a component type with its domain and lifecycle functions.
    pub(crate) fn register<T: Component>(&mut self, domain: SemanticDomain) {
        self.mapping.insert(
            TypeId::of::<T>(),
            ComponentVTable {
                domain,
                layout: LayoutPolicy::Soa,
                create_column: || Box::new(Vec::<T>::new()),
                copy_row: |src_col, src_row, dest_col| {
                    // SAFETY: The registry keys this vtable by `TypeId::of::<T>()`. Callers
                    // (the `World` migration / clone paths) only invoke `copy_row` when the
                    // dynamic types of `src_col` and `dest_col` are both `Vec<T>` for the
                    // same `T` this vtable was registered against — guaranteed by their
                    // matching TypeId lookup. The two downcasts therefore always succeed.
                    // `src_row` is required by the caller contract to satisfy
                    // `src_row < src_vec.len()`; entity locations stored on the World only
                    // ever point to valid rows in their registered pages, so
                    // `get_unchecked` is sound here.
                    unsafe {
                        let src_vec = src_col.as_any().downcast_ref::<Vec<T>>().unwrap();
                        let dest_vec = dest_col.as_any_mut().downcast_mut::<Vec<T>>().unwrap();
                        dest_vec.push(src_vec.get_unchecked(src_row).clone());
                    }
                },
            },
        );
        // Ensure an access-counter slot exists for this component type.
        self.access.entry(TypeId::of::<T>()).or_default();
    }

    /// Looks up the `SemanticDomain` for a given `TypeId`.
    pub fn get_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.mapping.get(&type_id).map(|vtable| vtable.domain)
    }

    /// Returns the [`LayoutPolicy`] a component is currently stored with.
    pub fn layout_of(&self, type_id: TypeId) -> Option<LayoutPolicy> {
        self.mapping.get(&type_id).map(|v| v.layout)
    }

    /// Sets the intended [`LayoutPolicy`] for a component. The actual repack of
    /// stored columns is performed by the layout-adaptation pass; this only
    /// records the target.
    pub fn set_layout(&mut self, type_id: TypeId, layout: LayoutPolicy) {
        if let Some(v) = self.mapping.get_mut(&type_id) {
            v.layout = layout;
        }
    }

    /// Records one access: increments the query count and adds `rows` to the
    /// scanned total for every component in `type_ids`. Lock-free, called once
    /// per query — never per element.
    pub fn record_access(&self, type_ids: &[TypeId], rows: u64) {
        for tid in type_ids {
            if let Some(c) = self.access.get(tid) {
                c.query_count.fetch_add(1, Ordering::Relaxed);
                c.rows_scanned.fetch_add(rows, Ordering::Relaxed);
            }
        }
    }

    /// Returns `(query_count, rows_scanned)` for a component, if registered.
    pub fn access_stats(&self, type_id: TypeId) -> Option<(u64, u64)> {
        self.access.get(&type_id).map(|c| {
            (
                c.query_count.load(Ordering::Relaxed),
                c.rows_scanned.load(Ordering::Relaxed),
            )
        })
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

    /// (Internal) Creates empty columns for a given type signature.
    pub(crate) fn create_columns_for_signature(
        &self,
        signature: &[TypeId],
    ) -> HashMap<TypeId, Box<dyn AnyVec>> {
        let mut columns = HashMap::new();
        for type_id in signature {
            let constructor = self.get_column_constructor(type_id).unwrap();
            columns.insert(*type_id, constructor());
        }
        columns
    }
}

/// Inventory entry submitted by `#[derive(Component)]` when the type carries a
/// `#[component(domain = ...)]` attribute.
///
/// Each entry registers one concrete component type into a [`crate::ecs::World`]
/// with its declared [`SemanticDomain`], so domain assignment is a **property of
/// the type** (single source of truth) instead of a hand-maintained list in
/// `World::new`. Collected via [`inventory`] and replayed by `World::new`.
pub struct ComponentDomainRegistration {
    /// Registers the component into `world` (calls `World::register_component`).
    pub register: fn(&mut crate::ecs::World),
}

inventory::collect!(ComponentDomainRegistration);

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
