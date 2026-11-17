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

//! The `ComponentRegistry`: one vtable per component a world stores.

use crate::ecs::column_ops::RustColumns;
use crate::ecs::{AnyVec, ColumnMap, ColumnOps, Component, ComponentKey, RegisterError};
use khora_core::script::FieldHints;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::{
    any::{self, TypeId},
    collections::HashMap,
};

pub use super::component_domain_registration::ComponentDomainRegistration;
pub use khora_core::ecs::SemanticDomain;

/// Who writes a component, and so what the engine may do with it on the author's behalf.
///
/// | variant | offered in *Add Component* | copied on duplicate | saved |
/// |---|---|---|---|
/// | [`Authored`](Self::Authored) | yes | yes | in scenes |
/// | [`ToolAuthored`](Self::ToolAuthored) | no | yes | in scenes |
/// | [`Derived`](Self::Derived) | no | no | never |
/// | [`Runtime`](Self::Runtime) | no | no | in game saves, if `resumable` |
///
/// See the book: concepts/ecs.md, "Provenance".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ComponentProvenance {
    /// Written by a human through the editor or by game code. The default.
    #[default]
    Authored,

    /// Written by a tool action rather than by hand, such as `Parent` set by
    /// dragging in the scene tree.
    ToolAuthored,

    /// Recomputed by the engine from authored state, such as `GlobalTransform`.
    Derived,

    /// Per-run state no one authors and nothing recomputes, such as `PhysicsDebugData`.
    Runtime,
}

impl ComponentProvenance {
    /// Returns whether the editor offers this component in *Add Component*.
    pub const fn is_hand_authorable(self) -> bool {
        matches!(self, ComponentProvenance::Authored)
    }

    /// Returns whether duplicating an entity copies this component verbatim.
    pub const fn is_copied_on_duplicate(self) -> bool {
        matches!(
            self,
            ComponentProvenance::Authored | ComponentProvenance::ToolAuthored
        )
    }
}

/// How a component column is physically laid out in memory.
///
/// Every component is laid out `Soa` today: AGDF's online repack, which would
/// adapt it, is deferred, so the policy changes no behaviour yet. See the book:
/// concepts/agdf.md, "Layer 2 — the field-SoA substrate, and why it is the lever".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPolicy {
    /// Structure-of-Arrays: one contiguous `Vec<T>`.
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
/// Updated once per query, never per row; read to decide whether a layout
/// repack would pay off. Atomic, so they are recorded through `&self`.
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

/// A component's vtable: what storage knows about it and does with its columns.
///
/// See the book: concepts/ecs.md, "The storage model".
pub struct ComponentVTable {
    /// The key a page's columns know it by.
    pub key: ComponentKey,
    /// The one name, short: what scenes, the editor and scripts use.
    pub name: Arc<str>,
    /// Its Rust type, `None` for a declared component.
    pub rust: Option<RustIdentity>,
    /// The semantic domain whose pages hold it.
    pub domain: SemanticDomain,
    /// The current physical layout of its columns.
    pub layout: LayoutPolicy,
    /// Who writes it.
    pub provenance: ComponentProvenance,
    /// Whether a game save keeps it although a scene does not.
    pub resumable: bool,
    /// The part it plays for scripts, `None` for a component that plays none.
    pub script: Option<crate::ecs::ScriptRole>,
    /// The module whose script declares it, `None` for a Rust component or an orphan.
    ///
    /// An orphan is one only a file declared, or one no script declares any more.
    pub declared_by: Option<String>,
    /// How the inspector presents its fields, by field name, for those given a hint.
    ///
    /// Editor-only: never part of the layout, a scene or a snapshot.
    pub field_hints: Vec<(String, FieldHints)>,
    /// What storage does with its columns.
    pub columns: Arc<dyn ColumnOps>,
}

impl ComponentVTable {
    /// Returns the hints the field `field` is given, if any.
    pub fn hints(&self, field: &str) -> Option<&FieldHints> {
        self.field_hints
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, hints)| hints)
    }
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
/// Found by key or by short name. Rust components are registered as the
/// world is built; declared components while it runs.
#[derive(Debug, Default)]
pub struct ComponentRegistry {
    vtables: HashMap<ComponentKey, ComponentVTable>,
    by_name: HashMap<Arc<str>, ComponentKey>,
    /// Access-pattern counters, read by layout adaptation.
    access: HashMap<ComponentKey, AccessCounters>,
    /// Moves whenever what a load reads of the registry changes.
    generation: u64,
    /// `generation`, published for a load prepared off the frame to watch.
    published: Arc<std::sync::atomic::AtomicU64>,
}

impl ComponentRegistry {
    /// Returns the vtable of component `key`, if registered.
    pub fn vtable(&self, key: ComponentKey) -> Option<&ComponentVTable> {
        self.vtables.get(&key)
    }

    /// Returns the key of the component whose short name is `name`, if registered.
    pub fn key_named(&self, name: &str) -> Option<ComponentKey> {
        self.by_name.get(name).copied()
    }

    /// Returns the registry's generation, which a load prepared against it reads.
    ///
    /// Moves whenever a component is registered or declared, relaid out,
    /// moved to another domain, or given another provenance.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns [`generation`](Self::generation), shared and kept up to date.
    ///
    /// A load prepared off the frame reads it to learn the registry moved since.
    pub(crate) fn published_generation(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.published)
    }

    /// Returns the number of registered components.
    pub fn len(&self) -> usize {
        self.vtables.len()
    }

    /// Returns whether no component is registered.
    pub fn is_empty(&self) -> bool {
        self.vtables.is_empty()
    }

    /// Returns every registered component, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &ComponentVTable> {
        self.vtables.values()
    }

    /// Registers the Rust component `T` in `domain`; again in the same domain, does nothing.
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
            resumable: false,
            script: None,
            declared_by: None,
            field_hints: Vec::new(),
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
        resumable: bool,
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
            resumable,
            script: None,
            declared_by: None,
            field_hints: Vec::new(),
            columns,
        });
        Ok(key)
    }

    fn insert(&mut self, vtable: ComponentVTable) {
        move_generation(&mut self.generation, &self.published);
        self.by_name.insert(vtable.name.clone(), vtable.key);
        self.access.entry(vtable.key).or_default();
        self.vtables.insert(vtable.key, vtable);
    }

    /// Records the part component `key` plays for scripts.
    pub(crate) fn set_script_role(
        &mut self,
        key: ComponentKey,
        role: Option<crate::ecs::ScriptRole>,
    ) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            vtable.script = role;
        }
    }

    /// Records what keeps component `key`: scenes, by its provenance, and
    /// game saves, when resumable.
    pub(crate) fn set_provenance(
        &mut self,
        key: ComponentKey,
        provenance: ComponentProvenance,
        resumable: bool,
    ) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            if vtable.provenance != provenance || vtable.resumable != resumable {
                move_generation(&mut self.generation, &self.published);
            }
            vtable.provenance = provenance;
            vtable.resumable = resumable;
        }
    }

    /// Records how the inspector presents component `key`'s fields.
    pub(crate) fn set_field_hints(&mut self, key: ComponentKey, hints: Vec<(String, FieldHints)>) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            vtable.field_hints = hints;
        }
    }

    /// Records which module's script declares component `key` now.
    pub(crate) fn set_declared_by(&mut self, key: ComponentKey, module: Option<String>) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            vtable.declared_by = module;
        }
    }

    /// Replaces a declared component's column operations — its new fields.
    pub(crate) fn set_columns(&mut self, key: ComponentKey, columns: Arc<dyn ColumnOps>) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            move_generation(&mut self.generation, &self.published);
            vtable.columns = columns;
        }
    }

    /// Moves a declared component to `domain`; its rows are the caller's to
    /// move.
    pub(crate) fn set_domain(&mut self, key: ComponentKey, domain: SemanticDomain) {
        if let Some(vtable) = self.vtables.get_mut(&key) {
            if vtable.domain != domain {
                move_generation(&mut self.generation, &self.published);
            }
            vtable.domain = domain;
        }
    }

    /// Returns the domain of component `key`, if registered.
    pub fn domain_of(&self, key: ComponentKey) -> Option<SemanticDomain> {
        self.vtables.get(&key).map(|vtable| vtable.domain)
    }

    /// Returns the `SemanticDomain` of the Rust component `type_id`, if registered.
    pub fn get_domain(&self, type_id: TypeId) -> Option<SemanticDomain> {
        self.domain_of(ComponentKey::Rust(type_id))
    }

    /// Returns the [`LayoutPolicy`] a component is currently stored with.
    pub fn layout_of(&self, type_id: TypeId) -> Option<LayoutPolicy> {
        self.vtables
            .get(&ComponentKey::Rust(type_id))
            .map(|vtable| vtable.layout)
    }

    /// Records the [`LayoutPolicy`] a component should be stored with.
    ///
    /// Stored columns are not repacked here.
    pub fn set_layout(&mut self, type_id: TypeId, layout: LayoutPolicy) {
        if let Some(vtable) = self.vtables.get_mut(&ComponentKey::Rust(type_id)) {
            vtable.layout = layout;
        }
    }

    /// Records one query visiting `rows` rows of every component in `keys`.
    ///
    /// Lock-free; called once per query, never per row.
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

    /// Returns the size of one row of a registered component, in bytes.
    pub fn size_of(&self, type_id: TypeId) -> Option<usize> {
        self.vtables
            .get(&ComponentKey::Rust(type_id))
            .map(|vtable| vtable.columns.size_bytes())
    }

    /// Returns every component's `(name, size_bytes, query_count, rows_scanned)`.
    ///
    /// What the DCC's layout advisor reads; allocates one entry per component.
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

    /// Creates the empty columns of a signature, through each component's vtable.
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

    /// Copies row `row` of `src` onto the end of `dst`, both columns of component `key`.
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

/// Returns a type's name with every path stripped, generics included.
///
/// `khora_data::ecs::HandleComponent<khora_core::asset::Mesh>` is `HandleComponent<Mesh>`.
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

/// Moves a registry's `generation`, and publishes it.
fn move_generation(generation: &mut u64, published: &std::sync::atomic::AtomicU64) {
    *generation += 1;
    published.store(*generation, std::sync::atomic::Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::ComponentProvenance;
    use super::ComponentProvenance::*;

    /// Only author-written components reach the *Add Component* menu.
    #[test]
    fn only_authored_is_hand_authorable() {
        assert!(Authored.is_hand_authorable());
        assert!(!ToolAuthored.is_hand_authorable());
        assert!(!Derived.is_hand_authorable());
        assert!(!Runtime.is_hand_authorable());
    }

    /// Duplication copies what a human or a tool put there; `ToolAuthored` is
    /// copied though not offered.
    #[test]
    fn engine_written_components_are_not_copied() {
        assert!(Authored.is_copied_on_duplicate());
        assert!(ToolAuthored.is_copied_on_duplicate());
        assert!(!Derived.is_copied_on_duplicate());
        assert!(!Runtime.is_copied_on_duplicate());
    }

    /// A component that names no provenance is the author's data.
    #[test]
    fn default_is_authored() {
        assert_eq!(ComponentProvenance::default(), Authored);
    }
}
