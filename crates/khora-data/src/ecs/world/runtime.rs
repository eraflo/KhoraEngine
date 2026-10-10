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

//! Components declared while the engine runs: registering them, changing
//! their fields, and reading and writing their rows by key.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::component_access::AddComponentError;
use super::World;
use crate::ecs::packed::{FieldError, PackedColumn, PackedColumns, PackedLayout};
use crate::ecs::page::PageIndex;
use crate::ecs::{
    AnyVec, Component, ComponentKey, ComponentProvenance, ComponentRegistry, ComponentVTable,
    SemanticDomain,
};
use crate::scene::component_registration::registration_for;

/// A component declared while the engine runs, known only by its fields.
#[derive(Debug, Clone)]
pub struct RuntimeComponentDecl {
    /// Its name — unique among every component of the `World`.
    pub name: String,
    /// The domain whose pages hold it.
    pub domain: SemanticDomain,
    /// Who writes it.
    pub provenance: ComponentProvenance,
    /// Its fields.
    pub layout: PackedLayout,
}

/// What a [`World::relayout`] did to the rows it rebuilt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelayoutReport {
    /// Fields the old layout had and the new one does not: their values are gone.
    pub dropped: Vec<String>,
    /// Fields that are new, or changed kind: every row holds their default.
    pub defaulted: Vec<String>,
}

/// Why a component could not be registered, or relaid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// Another component already has the name — a component's name is unique
    /// among every component, Rust or declared.
    NameTaken {
        /// The name asked for.
        name: String,
    },
    /// A declared component already has this name: relayout it instead.
    AlreadyRegistered {
        /// The name asked for.
        name: String,
        /// The component that has it.
        key: ComponentKey,
    },
    /// Two Rust types share one short name.
    DuplicateName {
        /// The short name.
        name: String,
        /// The full path of the type registered first.
        first: String,
        /// The full path of the type refused.
        second: String,
    },
    /// The Rust type is already registered in another domain.
    DomainConflict {
        /// The component.
        name: String,
        /// The domain it is registered in.
        existing: SemanticDomain,
        /// The domain asked for.
        requested: SemanticDomain,
    },
    /// A Rust component's fields are its type's: it cannot be relaid out.
    NotRuntime {
        /// The component.
        name: String,
    },
    /// Another declared name hashes to the same key — a 1-in-2^128 event,
    /// refused rather than letting two components share a column.
    KeyCollision {
        /// The name asked for.
        name: String,
        /// The name already holding the key.
        other: String,
    },
    /// No component has this key.
    Unknown(ComponentKey),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NameTaken { name } => write!(
                f,
                "`{name}` already names another component: a component's name is unique among                  Rust and declared components alike"
            ),
            Self::AlreadyRegistered { name, .. } => write!(
                f,
                "a run-time component is already named `{name}`: give it new fields with `relayout`"
            ),
            Self::DuplicateName {
                name,
                first,
                second,
            } => write!(
                f,
                "two components are named `{name}`: `{first}` and `{second}` — scenes, the editor and \
                 scripts know a component by that one name, so rename one of them"
            ),
            Self::DomainConflict {
                name,
                existing,
                requested,
            } => write!(
                f,
                "`{name}` is already registered in the {existing:?} domain; registering it in \
                 {requested:?} would hide every value stored under the first"
            ),
            Self::NotRuntime { name } => write!(
                f,
                "`{name}` is a Rust component: its fields are its type's"
            ),
            Self::KeyCollision { name, other } => write!(
                f,
                "`{name}` and `{other}` hash to the same component key: rename one of them"
            ),
            Self::Unknown(key) => write!(f, "no component has the key {key:?}"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Why a row could not be attached or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowError {
    /// The entity is not alive.
    NoSuchEntity(EntityId),
    /// No declared component has this key.
    Unknown(ComponentKey),
    /// The entity already holds this component.
    AlreadyAttached,
    /// A field refused its value.
    Field(FieldError),
    /// The value names a field the component does not have.
    UnknownField {
        /// The name written.
        field: String,
    },
    /// The value is not a component's fields (a `ScriptValue::Struct`) nor
    /// nothing (`Unit`).
    NotFields {
        /// The type of the value offered (`ScriptValue::type_name`).
        found: String,
    },
}

/// One entity's row of one component, read by id.
pub struct RowRef<'w> {
    vtable: &'w ComponentVTable,
    column: &'w dyn AnyVec,
    row: usize,
}

impl<'w> RowRef<'w> {
    /// The component.
    pub fn key(&self) -> ComponentKey {
        self.vtable.key
    }

    /// The component's vtable.
    pub fn vtable(&self) -> &'w ComponentVTable {
        self.vtable
    }

    /// The value of the field at `slot` — of a run-time component; a Rust
    /// component's fields are read through its type.
    pub fn field(&self, slot: usize) -> Option<ScriptValue> {
        packed(self.column)?.get(self.row, slot)
    }
}

/// One entity's row of one component, written by id.
pub struct RowMut<'w> {
    vtable: &'w ComponentVTable,
    column: &'w mut dyn AnyVec,
    row: usize,
}

impl<'w> RowMut<'w> {
    /// The component.
    pub fn key(&self) -> ComponentKey {
        self.vtable.key
    }

    /// The component's vtable.
    pub fn vtable(&self) -> &ComponentVTable {
        self.vtable
    }

    /// The value of the field at `slot` — of a run-time component.
    pub fn field(&self, slot: usize) -> Option<ScriptValue> {
        packed(&*self.column)?.get(self.row, slot)
    }

    /// Writes the field at `slot` of a run-time component, refusing a value
    /// not of its kind.
    ///
    /// # Panics
    ///
    /// When `slot` is not a field of the component's layout, or the row is a
    /// Rust component's: a slot comes from the layout the row is read with.
    pub fn set_field(&mut self, slot: usize, value: &ScriptValue) -> Result<(), FieldError> {
        let row = self.row;
        let column = self
            .column
            .as_any_mut()
            .downcast_mut::<PackedColumn>()
            .expect("a field is written by slot on a declared component's row");
        assert!(
            column.has(row, slot),
            "slot {slot} is not a field of this row"
        );
        column.set(row, slot, value)
    }
}

/// `column` as a run-time component's.
fn packed(column: &dyn AnyVec) -> Option<&PackedColumn> {
    column.as_any().downcast_ref::<PackedColumn>()
}

impl World {
    /// Every component this world knows.
    pub fn components(&self) -> &ComponentRegistry {
        &self.storage.registry
    }

    /// Registers the Rust component `T` in `domain`, refusing a short name
    /// another Rust type has. Registering it again in the domain it has
    /// returns its id.
    pub fn try_register_component<T: Component>(
        &mut self,
        domain: SemanticDomain,
    ) -> Result<ComponentKey, RegisterError> {
        let known = self.storage.registry.len();
        // The persistence half of the component — when it has one — says who
        // writes it.
        let provenance = registration_for(TypeId::of::<T>())
            .map_or(ComponentProvenance::Authored, |reg| reg.provenance);
        let key = self.storage.registry.register::<T>(domain, provenance)?;
        if self.storage.registry.len() != known {
            // A plan names the domains its components live in; one made
            // before this registration would keep looking for `T` where it
            // is not.
            self.planner
                .query_cache
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        Ok(key)
    }

    /// Registers a component declared while the engine runs, under the key
    /// of its name — the same key in every world. Called at the frame
    /// boundary, where the `World` is mutable.
    pub fn register_runtime_component(
        &mut self,
        decl: RuntimeComponentDecl,
    ) -> Result<ComponentKey, RegisterError> {
        let columns = Arc::new(PackedColumns::new(Arc::new(decl.layout)));
        self.storage
            .registry
            .register_declared(&decl.name, decl.domain, decl.provenance, columns)
    }

    /// Gives a declared component new fields, rebuilding every row of it field
    /// by field, by name: a field keeps its value where its name and kind are
    /// unchanged, and takes its default where it is new or changed kind.
    pub fn relayout(
        &mut self,
        key: ComponentKey,
        layout: PackedLayout,
    ) -> Result<RelayoutReport, RegisterError> {
        let vtable = self
            .storage
            .registry
            .vtable(key)
            .ok_or(RegisterError::Unknown(key))?;
        let Some(old) = vtable.columns.packed() else {
            return Err(RegisterError::NotRuntime {
                name: vtable.name.to_string(),
            });
        };
        let report = RelayoutReport {
            dropped: old
                .fields()
                .iter()
                .filter(|field| layout.slot_of(&field.name).is_none())
                .map(|field| field.name.clone())
                .collect(),
            defaulted: layout
                .fields()
                .iter()
                .filter(|field| {
                    old.slot_of(&field.name)
                        .is_none_or(|slot| old.fields()[slot].kind != field.kind)
                })
                .map(|field| field.name.clone())
                .collect(),
        };
        if !report.dropped.is_empty() {
            log::warn!(
                "`{}` lost the fields {:?}: their values are gone",
                vtable.name,
                report.dropped
            );
        }
        let domain = vtable.domain;
        let layout = Arc::new(layout);
        for page in &mut self.storage.pages {
            let Some(column) = page.columns.get_mut(&key) else {
                continue;
            };
            if let Some(packed) = packed(column.as_ref()) {
                *column = Box::new(packed.relaid(&layout));
            }
        }
        self.storage
            .registry
            .set_columns(key, Arc::new(PackedColumns::new(layout)));
        self.bump_domain_epoch(domain);
        Ok(report)
    }

    /// Attaches a declared component: its defaults, then `value`'s fields (a
    /// `ScriptValue::Struct`, or `Unit`). Nothing is attached when a field
    /// refuses its value.
    pub fn add_runtime_component(
        &mut self,
        entity: EntityId,
        key: ComponentKey,
        value: &ScriptValue,
    ) -> Result<(), RowError> {
        if !self.contains(entity) {
            return Err(RowError::NoSuchEntity(entity));
        }
        let layout = self
            .storage
            .registry
            .vtable(key)
            .and_then(|vtable| vtable.columns.packed())
            .ok_or(RowError::Unknown(key))?;
        // Checked whole before anything moves: a refused value attaches nothing.
        let patch: Vec<(usize, &ScriptValue)> = match value {
            ScriptValue::Unit => Vec::new(),
            ScriptValue::Struct(fields) => fields
                .iter()
                .map(|(name, value)| {
                    let slot = layout.slot_of(name).ok_or_else(|| RowError::UnknownField {
                        field: name.clone(),
                    })?;
                    let field = &layout.fields()[slot];
                    if field.kind.holds(value) {
                        Ok((slot, value))
                    } else {
                        Err(RowError::Field(FieldError {
                            field: field.name.clone(),
                            expected: field.kind,
                            found: value.type_name().to_owned(),
                        }))
                    }
                })
                .collect::<Result<_, _>>()?,
            other => {
                return Err(RowError::NotFields {
                    found: other.type_name().to_owned(),
                })
            }
        };
        let attached = self.attach(entity, key, |column| {
            let column = column
                .as_any_mut()
                .downcast_mut::<PackedColumn>()
                .expect("a declared component's column is packed");
            column.push_defaults();
            let row = column.len() - 1;
            for (slot, value) in patch {
                // Checked of its kind above.
                let _ = column.set(row, slot, value);
            }
        });
        match attached {
            Ok(_) => Ok(()),
            Err(AddComponentError::ComponentAlreadyExists) => Err(RowError::AlreadyAttached),
            Err(AddComponentError::EntityNotFound) => Err(RowError::NoSuchEntity(entity)),
            Err(AddComponentError::ComponentNotRegistered) => Err(RowError::Unknown(key)),
        }
    }

    /// Detaches component `key` from `entity` — a Rust component or a
    /// declared one; `false` when it held none.
    pub fn remove_component_by_key(&mut self, entity: EntityId, key: ComponentKey) -> bool {
        self.detach(entity, key).is_ok()
    }

    /// `entity`'s row of component `key`.
    pub fn row(&self, entity: EntityId, key: ComponentKey) -> Option<RowRef<'_>> {
        let (vtable, location) = self.row_location(entity, key)?;
        let column = self.storage.pages[location.page_id as usize]
            .columns
            .get(&key)?;
        Some(RowRef {
            vtable,
            column: column.as_ref(),
            row: location.row_index as usize,
        })
    }

    /// `entity`'s row of component `key`, writable. Marks the component's
    /// domain changed, as handing out any writable access does.
    pub fn row_mut(&mut self, entity: EntityId, key: ComponentKey) -> Option<RowMut<'_>> {
        let (vtable, location) = self.row_location(entity, key)?;
        let domain = vtable.domain;
        if !self.storage.pages[location.page_id as usize]
            .columns
            .contains_key(&key)
        {
            return None;
        }
        self.bump_domain_epoch(domain);
        let storage = &mut self.storage;
        let vtable = storage.registry.vtable(key)?;
        let column = storage.pages[location.page_id as usize]
            .columns
            .get_mut(&key)?;
        Some(RowMut {
            vtable,
            column: column.as_mut(),
            row: location.row_index as usize,
        })
    }

    /// The vtable of `key` and `entity`'s live row in its domain.
    fn row_location(
        &self,
        entity: EntityId,
        key: ComponentKey,
    ) -> Option<(&ComponentVTable, PageIndex)> {
        let vtable = self.storage.registry.vtable(key)?;
        let (slot, metadata) = self.entities.get(entity.index as usize)?;
        if *slot != entity {
            return None;
        }
        let location = *metadata.as_ref()?.locations.get(&vtable.domain)?;
        Some((vtable, location))
    }
}
