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

use crate::ecs::column_ops::RustColumns;
use crate::ecs::{AnyVec, ColumnMap, ColumnOps, Component, ComponentKey, RegisterError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::{
    any::{self, TypeId},
    collections::HashMap,
};

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
    /// For components related to audio, such as audio sources and listeners.
    Audio,
    /// For components related to physics simulation.
    Physics,
    /// For components driving the in-world UI subsystem (UiNode, UiText, etc).
    Ui,
    /// For gameplay logic attached to entities — which behavior an entity runs
    /// and the state that behavior keeps.
    ///
    /// Its own domain rather than a corner of `Spatial` because the two change
    /// on different clocks: a transform moves every frame, while which script an
    /// entity runs changes when a designer edits the scene. Sharing an epoch
    /// would mean one invalidates the other's cached projection for no reason.
    Script,
}

impl SemanticDomain {
    /// Number of semantic domains — sizes fixed per-domain tables such as the
    /// [`World`](crate::ecs::World)'s change epochs.
    pub const COUNT: usize = 6;

    /// Dense index of this domain in `0..COUNT`, used to address fixed
    /// per-domain arrays without a `HashMap` lookup.
    pub const fn index(self) -> usize {
        match self {
            SemanticDomain::Spatial => 0,
            SemanticDomain::Render => 1,
            SemanticDomain::Audio => 2,
            SemanticDomain::Physics => 3,
            SemanticDomain::Ui => 4,
            SemanticDomain::Script => 5,
        }
    }
}

/// Who is allowed to write a component — the axis orthogonal to
/// [`SemanticDomain`].
///
/// `SemanticDomain` answers *which subsystem consumes this data* and drives
/// change epochs, page grouping and `Flow` gating. It deliberately says nothing
/// about **authorship**: `Transform` and `GlobalTransform` are both `Spatial`,
/// yet one is written by a human and the other is recomputed every frame by
/// `transform_propagation`. `Collider` and `PhysicsDebugData` are both
/// `Physics`, yet one is an input and the other is debug output.
///
/// Without this axis the same intent gets re-encoded ad hoc at every site that
/// needs it — which component to offer in "Add Component", which to copy when
/// duplicating an entity, which to write to a scene file. Each such list is
/// hand-maintained, so none of them covers components defined outside this
/// crate.
///
/// The four variants encode two independent bits:
///
/// | variant | offered to the author | copied on duplicate |
/// |---|---|---|
/// | [`Authored`](Self::Authored) | yes | yes |
/// | [`ToolAuthored`](Self::ToolAuthored) | no | yes |
/// | [`Derived`](Self::Derived) | no | no |
/// | [`Runtime`](Self::Runtime) | no | no |
///
/// It also decides what is saved: a scene holds `Authored` and `ToolAuthored`
/// components — `Parent` must persist even though nobody adds it by hand — and
/// never `Derived` or `Runtime` ones; a game save also keeps a `Runtime`
/// component declared `resumable`. `#[component(skip)]` leaves a field out of
/// what is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ComponentProvenance {
    /// Written by a human through the editor or by game code. Belongs in a
    /// scene file, survives duplication, and is offered in "Add Component".
    /// This is the default for any component that does not say otherwise.
    #[default]
    Authored,

    /// Written by a tool action rather than by hand — `Parent`, whose edge is
    /// set by dragging in the scene tree. It persists and must survive
    /// duplication, but adding an empty one by hand is meaningless, so it is
    /// not offered in the menu.
    ToolAuthored,

    /// Recomputed by the engine from `Authored` state — `GlobalTransform` from
    /// `Transform` + `Parent`, or the `HandleComponent<Gpu*>` projections from
    /// their asset handles. A duplicate must **not** carry a copy: the engine
    /// regenerates it, and a stale copy would be wrong until it did.
    Derived,

    /// Per-run transient state that no one authors and nothing recomputes from
    /// authored data — `PhysicsDebugData`. Never offered, never copied.
    Runtime,
}

impl ComponentProvenance {
    /// Whether the editor should offer this component in "+ Add Component".
    ///
    /// Only [`Authored`](Self::Authored) qualifies: everything else is written
    /// by the engine or by a tool action.
    pub const fn is_hand_authorable(self) -> bool {
        matches!(self, ComponentProvenance::Authored)
    }

    /// Whether duplicating an entity should copy this component verbatim.
    ///
    /// `Derived` and `Runtime` are excluded because the engine produces them:
    /// copying would install a stale value that the next tick overwrites at
    /// best, and that reads as corrupt state at worst.
    pub const fn is_copied_on_duplicate(self) -> bool {
        matches!(
            self,
            ComponentProvenance::Authored | ComponentProvenance::ToolAuthored
        )
    }
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

/// Online access-pattern counters for one component.
///
/// Updated **once per query** (off the per-element hot path), read by the DCC /
/// telemetry to decide whether a memory-layout repack would pay off. Atomics so
/// they can be recorded through `&self`. One small entry per component (not
/// per entity): the memory cost is constant, not proportional to the world.
#[derive(Debug, Default)]
pub struct AccessCounters {
    /// Number of queries that touched this component.
    pub query_count: AtomicU64,
    /// Cumulative rows visited across those queries.
    pub rows_scanned: AtomicU64,
}

/// The Rust half of a component's identity — absent for a declared component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RustIdentity {
    /// The type's `TypeId`.
    pub type_id: TypeId,
    /// The type's full path (`std::any::type_name`).
    pub path: &'static str,
}

/// A component's vtable: what storage knows about it and does with its
/// columns, registered once per component.
///
/// Pages hold type-erased columns; this table says which domain's pages hold
/// the component, how its columns are laid out, and — through
/// [`columns`](Self::columns) — how to create one and move a row between
/// pages, without storage ever knowing the column's type.
pub struct ComponentVTable {
    /// The key a page's columns know it by.
    pub key: ComponentKey,
    /// The one name, short: what scenes, the editor and scripts use.
    pub name: Arc<str>,
    /// Its Rust type, `None` for a declared component.
    pub rust: Option<RustIdentity>,
    /// The semantic domain whose pages hold it.
    pub domain: SemanticDomain,
    /// Current physical layout of its columns (default `Soa`).
    pub layout: LayoutPolicy,
    /// Who writes it.
    pub provenance: ComponentProvenance,
    /// What storage does with its columns.
    pub columns: Arc<dyn ColumnOps>,
}

impl std::fmt::Debug for ComponentVTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentVTable")
            .field("key", &self.key)
            .field("name", &self.name)
            .field("rust", &self.rust)
            .field("domain", &self.domain)
            .field("layout", &self.layout)
            .field("provenance", &self.provenance)
            .finish_non_exhaustive()
    }
}

/// Every component a `World` stores: one vtable each, by key.
///
/// A component is found by its key — its `TypeId`, or its declared name's
/// hash — or by its one short name. Rust components are registered as the
/// world is built; declared components while it runs.
#[derive(Debug, Default)]
pub struct ComponentRegistry {
    vtables: HashMap<ComponentKey, ComponentVTable>,
    by_name: HashMap<Arc<str>, ComponentKey>,
    /// Per-component online access-pattern counters (layout adaptation).
    access: HashMap<ComponentKey, AccessCounters>,
}

impl ComponentRegistry {
    /// The vtable of component `key`, if registered.
    pub fn vtable(&self, key: ComponentKey) -> Option<&ComponentVTable> {
        self.vtables.get(&key)
    }

    /// The key of the component named `name` (its short name), if registered.
    pub fn key_named(&self, name: &str) -> Option<ComponentKey> {
        self.by_name.get(name).copied()
    }

    /// The number of registered components.
    pub fn len(&self) -> usize {
        self.vtables.len()
    }

    /// Whether no component is registered.
    pub fn is_empty(&self) -> bool {
        self.vtables.is_empty()
    }

    /// Every registered component, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &ComponentVTable> {
        self.vtables.values()
    }

    /// Registers the Rust component `T` in `domain`. Registering it again in
    /// the domain it has changes nothing.
    pub(crate) fn register<T: Component>(
        &mut self,
        domain: SemanticDomain,
        provenance: ComponentProvenance,
    ) -> Result<ComponentKey, RegisterError> {
        let key = ComponentKey::of::<T>();
        let path = any::type_name::<T>();
        let name = short_name(path);
        if let Some(existing) = self.vtables.get(&key) {
            return if existing.domain == domain {
                Ok(key)
            } else {
                Err(RegisterError::DomainConflict {
                    name,
                    existing: existing.domain,
                    requested: domain,
                })
            };
        }
        if let Some(taken) = self.key_named(&name) {
            return Err(match self.vtables[&taken].rust {
                Some(first) => RegisterError::DuplicateName {
                    name,
                    first: first.path.to_owned(),
                    second: path.to_owned(),
                },
                None => RegisterError::NameTaken { name },
            });
        }
        self.insert(ComponentVTable {
            key,
            name: name.into(),
            rust: Some(RustIdentity {
                type_id: TypeId::of::<T>(),
                path,
            }),
            domain,
            layout: LayoutPolicy::Soa,
            provenance,
            columns: Arc::new(RustColumns::<T>::new()),
        });
        Ok(key)
    }

    /// Registers a component declared while the engine runs, under the key of
    /// its name.
    pub(crate) fn register_declared(
        &mut self,
        name: &str,
        domain: SemanticDomain,
        provenance: ComponentProvenance,
        columns: Arc<dyn ColumnOps>,
    ) -> Result<ComponentKey, RegisterError> {
        if let Some(taken) = self.key_named(name) {
            return Err(match self.vtables[&taken].rust {
                Some(_) => RegisterError::NameTaken {
                    name: name.to_owned(),
                },
                None => RegisterError::AlreadyRegistered {
                    name: name.to_owned(),
                    key: taken,
                },
            });
        }
        let key = ComponentKey::named(name);
        if let Some(other) = self.vtables.get(&key) {
            return Err(RegisterError::KeyCollision {
                name: name.to_owned(),
                other: other.name.to_string(),
            });
        }
        self.insert(ComponentVTable {
            key,
            name: name.into(),
            rust: None,
            domain,
            layout: LayoutPolicy::Soa,
            provenance,
            columns,
        });
        Ok(key)
    }

    fn insert(&mut self, vtable: ComponentVTable) {
        self.by_name.insert(vtable.name.clone(), vtable.key);
        self.access.entry(vtable.key).or_default();
        self.vtables.insert(vtable.key, vtable);
    }

    /// Replaces a declared component's column operations — its new fields.
    pub(crate) fn set_columns(&mut self, key: ComponentKey, columns: Arc<dyn ColumnOps>) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            vtable.columns = columns;
        }
    }

    /// The domain of component `key`, if registered.
    pub fn domain_of(&self, key: ComponentKey) -> Option<SemanticDomain> {
        self.vtables.get(&key).map(|vtable| vtable.domain)
    }

    /// Looks up the `SemanticDomain` of the Rust component `type_id`.
    pub fn get_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.domain_of(ComponentKey::Rust(type_id))
    }

    /// Returns the [`LayoutPolicy`] a component is currently stored with.
    pub fn layout_of(&self, type_id: TypeId) -> Option<LayoutPolicy> {
        self.vtables
            .get(&ComponentKey::Rust(type_id))
            .map(|vtable| vtable.layout)
    }

    /// Sets the intended [`LayoutPolicy`] for a component. The actual repack of
    /// stored columns is performed by the layout-adaptation pass; this only
    /// records the target.
    pub fn set_layout(&mut self, type_id: TypeId, layout: LayoutPolicy) {
        if let Some(vtable) = self.vtables.get_mut(&ComponentKey::Rust(type_id)) {
            vtable.layout = layout;
        }
    }

    /// Records one access: increments the query count and adds `rows` to the
    /// scanned total for every component in `keys`. Lock-free, called once
    /// per query — never per element.
    pub fn record_access(&self, keys: &[ComponentKey], rows: u64) {
        for key in keys {
            if let Some(c) = self.access.get(key) {
                c.query_count.fetch_add(1, Ordering::Relaxed);
                c.rows_scanned.fetch_add(rows, Ordering::Relaxed);
            }
        }
    }

    /// Returns `(query_count, rows_scanned)` for a component, if registered.
    pub fn access_stats(&self, type_id: TypeId) -> Option<(u64, u64)> {
        self.access.get(&ComponentKey::Rust(type_id)).map(|c| {
            (
                c.query_count.load(Ordering::Relaxed),
                c.rows_scanned.load(Ordering::Relaxed),
            )
        })
    }

    /// The size of one row of a registered component, or `None` if unregistered.
    pub fn size_of(&self, type_id: TypeId) -> Option<usize> {
        self.vtables
            .get(&ComponentKey::Rust(type_id))
            .map(|vtable| vtable.columns.size_bytes())
    }

    /// Snapshot of every registered component's `(name, size_bytes,
    /// query_count, rows_scanned)` — the input the DCC's layout advisor reads
    /// (read-only; observation tunnel). Allocates a small `Vec` (one entry per
    /// component), so it is cheap enough to sample off the hot path.
    pub fn access_snapshot(&self) -> Vec<(Arc<str>, usize, u64, u64)> {
        self.vtables
            .values()
            .filter_map(|vtable| {
                let c = self.access.get(&vtable.key)?;
                Some((
                    vtable.name.clone(),
                    vtable.columns.size_bytes(),
                    c.query_count.load(Ordering::Relaxed),
                    c.rows_scanned.load(Ordering::Relaxed),
                ))
            })
            .collect()
    }

    /// (Internal) Creates the empty columns of a signature, through each
    /// component's vtable.
    pub(crate) fn create_columns_for_signature(&self, signature: &[ComponentKey]) -> ColumnMap {
        signature
            .iter()
            .map(|key| {
                let vtable = self
                    .vtables
                    .get(key)
                    .expect("a page signature names registered components only");
                (*key, vtable.columns.create_column())
            })
            .collect()
    }

    /// (Internal) Copies row `row` of `src` onto the end of `dst`, both
    /// columns of component `key`.
    pub(crate) fn copy_row(
        &self,
        key: ComponentKey,
        src: &dyn AnyVec,
        row: usize,
        dst: &mut dyn AnyVec,
    ) {
        if let Some(vtable) = self.vtables.get(&key) {
            vtable.columns.copy_row(src, row, dst);
        }
    }
}

/// A type's name with every path stripped, generics included:
/// `khora_data::ecs::HandleComponent<khora_core::asset::Mesh>` is
/// `HandleComponent<Mesh>` — for a derived component, the name it was written
/// with.
pub(crate) fn short_name(path: &str) -> String {
    let mut name = String::with_capacity(path.len());
    let mut token = String::new();
    let flush = |token: &mut String, name: &mut String| {
        name.push_str(token.rsplit("::").next().unwrap_or(token));
        token.clear();
    };
    for c in path.chars() {
        if c.is_alphanumeric() || c == '_' || c == ':' {
            token.push(c);
        } else {
            flush(&mut token, &mut name);
            name.push(c);
        }
    }
    flush(&mut token, &mut name);
    name
}

/// Inventory entry submitted by `#[derive(Component)]` when the type carries a
/// `#[component(domain = ...)]` attribute.
///
/// Each entry registers one concrete component type into a [`crate::ecs::World`]
/// with its declared [`SemanticDomain`], so domain assignment is a **property of
/// the type** (single source of truth) instead of a hand-maintained list in
/// `World::new`. Collected via [`inventory`] and replayed by `World::new`.
///
/// It is the storage half of a Rust component; its persistence half is its
/// `ComponentRegistration`. The world's [`ComponentRegistry`] joins both into
/// one [`ComponentVTable`].
pub struct ComponentDomainRegistration {
    /// Registers the component into `world` (calls `World::register_component`).
    pub register: fn(&mut crate::ecs::World),
}

inventory::collect!(ComponentDomainRegistration);

#[cfg(test)]
mod provenance_tests {
    use super::ComponentProvenance;
    use super::ComponentProvenance::*;

    /// Only author-written components reach the "Add Component" menu — the
    /// whole point of the axis is that engine-written types opt out by
    /// construction rather than via a hand-maintained denylist.
    #[test]
    fn only_authored_is_hand_authorable() {
        assert!(Authored.is_hand_authorable());
        assert!(!ToolAuthored.is_hand_authorable());
        assert!(!Derived.is_hand_authorable());
        assert!(!Runtime.is_hand_authorable());
    }

    /// Duplication copies what a human or a tool put there, and lets the
    /// engine rebuild the rest. `ToolAuthored` is the variant that separates
    /// the two bits: not offered in the menu, but still copied.
    #[test]
    fn engine_written_components_are_not_copied() {
        assert!(Authored.is_copied_on_duplicate());
        assert!(ToolAuthored.is_copied_on_duplicate());
        assert!(!Derived.is_copied_on_duplicate());
        assert!(!Runtime.is_copied_on_duplicate());
    }

    /// A component says nothing about provenance unless it opts out, so the
    /// default has to be the author's data.
    #[test]
    fn default_is_authored() {
        assert_eq!(ComponentProvenance::default(), Authored);
    }
}
