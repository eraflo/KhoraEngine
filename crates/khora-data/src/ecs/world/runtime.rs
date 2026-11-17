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
//! their fields, attaching and removing them by key.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_core::sync::{LockExt, RwLockExt};

use super::component_access::AddComponentError;
use super::World;
use crate::ecs::packed::{retyped, FieldError, PackedColumn, PackedColumns, PackedLayout};
use crate::ecs::{
    AnyVec, Component, ComponentKey, ComponentProvenance, ComponentRegistry, SemanticDomain, Tick,
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
    /// Whether a game save keeps it although a scene does not, to resume play.
    pub resumable: bool,
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
    /// Another component, Rust or declared, already has the name.
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
    /// Another declared name hashes to the same key, so the two would share a column.
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
                "`{name}` already names another component: a component's name is unique among \
                 Rust and declared components alike"
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

/// Returns `column` as a run-time component's.
fn packed(column: &dyn AnyVec) -> Option<&PackedColumn> {
    column.as_any().downcast_ref::<PackedColumn>()
}

impl World {
    /// Returns every component this world knows.
    pub fn components(&self) -> &ComponentRegistry {
        &self.storage.registry
    }

    /// Returns the published generation of this world's load catalogue; every call shares it.
    ///
    /// A load prepared off the frame watches it to learn that the components
    /// it was prepared against changed.
    pub fn catalog_generation(&self) -> Arc<std::sync::atomic::AtomicU64> {
        self.storage.registry.published_generation()
    }

    /// Returns what preparing a load reads of this world, readable on any thread.
    ///
    /// Its generation moves whenever a component is registered, declared,
    /// relaid out, moved to another domain or given another provenance.
    pub fn load_catalog(&self) -> Arc<crate::scene::LoadCatalog> {
        let nowhere = self
            .nowhere
            .expect("a world makes its nowhere entity as it is created");
        let generation = self.storage.registry.generation();
        let mut cached = self.load_catalog.lock_recovered();
        match &*cached {
            // Built once per change of the components, so a load in a frame
            // (a prefab a script spawns) pays nothing more.
            Some(catalog) if catalog.generation() == generation => Arc::clone(catalog),
            _ => {
                let catalog = Arc::new(crate::scene::LoadCatalog::of(
                    &self.storage.registry,
                    nowhere,
                ));
                *cached = Some(Arc::clone(&catalog));
                catalog
            }
        }
    }

    /// Registers the Rust component `T` in `domain`, returning its key.
    ///
    /// Registering it again in the domain it has returns the same key.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] if another Rust type has its short name, or it is
    /// registered in another domain.
    pub fn try_register_component<T: Component>(
        &mut self,
        domain: SemanticDomain,
    ) -> Result<ComponentKey, RegisterError> {
        let known = self.storage.registry.len();
        // The component's persistence registration, if any, says who writes it.
        let provenance = registration_for(TypeId::of::<T>())
            .map_or(ComponentProvenance::Authored, |reg| reg.provenance);
        let key = self.storage.registry.register::<T>(domain, provenance)?;
        if self.storage.registry.len() != known {
            // A cached plan names the domains its components live in, so one
            // made before this registration would look for `T` where it is not.
            self.planner.query_cache.write_recovered().clear();
        }
        Ok(key)
    }

    /// Registers a component declared while the engine runs, keyed by its name in every world.
    pub fn register_runtime_component(
        &mut self,
        decl: RuntimeComponentDecl,
    ) -> Result<ComponentKey, RegisterError> {
        let mut layout = decl.layout;
        layout.kept_for(decl.provenance);
        let columns = Arc::new(PackedColumns::new(Arc::new(layout)));
        self.storage.registry.register_declared(
            &decl.name,
            decl.domain,
            decl.provenance,
            decl.resumable,
            columns,
        )
    }

    /// Gives a declared component new fields, rebuilding every row by field name.
    ///
    /// A field keeps its value where its name and kind are unchanged, or an
    /// `int` became a `float`; it takes its default where it is new, changed
    /// kind, or changed Ergon type (`string?` to `int?`).
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
                .enumerate()
                .filter(|(at, field)| {
                    old.slot_of(&field.name).is_none_or(|slot| {
                        !old.fields()[slot].kind.widens(field.kind)
                            || retyped(old.ty(slot), layout.ty(*at))
                    })
                })
                .map(|(_, field)| field.name.clone())
                .collect(),
        };
        if !report.dropped.is_empty() {
            log::warn!(
                "`{}` lost the fields {:?}: their values are gone",
                vtable.name,
                report.dropped
            );
        }
        let mut layout = layout;
        layout.kept_for(vtable.provenance);
        let layout = Arc::new(layout);
        let mut relaid = Vec::new();
        for (page_id, page) in self.storage.pages.iter_mut().enumerate() {
            let Some(column) = page.columns.get_mut(&key) else {
                continue;
            };
            if let Some(packed) = packed(column.as_ref()) {
                *column = Box::new(packed.relaid(&layout));
                if page.row_count() > 0 {
                    relaid.push(page_id as u32);
                }
            }
        }
        // Every row was rebuilt: every row is written.
        for page_id in relaid {
            self.stamp_column(page_id, key);
        }
        self.storage
            .registry
            .set_columns(key, Arc::new(PackedColumns::new(layout)));
        Ok(report)
    }

    /// Records the part declared component `key` plays for scripts, as its declaring module says.
    pub fn set_script_role(&mut self, key: ComponentKey, role: Option<crate::ecs::ScriptRole>) {
        self.storage.registry.set_script_role(key, role);
    }

    /// Records which module's script declares component `key`, `None` once none does.
    pub fn set_declared_by(&mut self, key: ComponentKey, module: Option<String>) {
        self.storage.registry.set_declared_by(key, module);
    }

    /// Records how the inspector presents declared component `key`'s hinted fields.
    ///
    /// Never relays the component out: hints are not its layout.
    pub fn set_field_hints(
        &mut self,
        key: ComponentKey,
        hints: Vec<(String, khora_core::script::FieldHints)>,
    ) {
        self.storage.registry.set_field_hints(key, hints);
    }

    /// Gives declared component `key` a new provenance and resumability, relaying out its rows.
    pub fn set_provenance(
        &mut self,
        key: ComponentKey,
        provenance: ComponentProvenance,
        resumable: bool,
    ) -> Result<(), RegisterError> {
        let Some(layout) = self
            .storage
            .registry
            .vtable(key)
            .and_then(|vtable| vtable.columns.packed())
            .cloned()
        else {
            return Err(RegisterError::Unknown(key));
        };
        self.storage
            .registry
            .set_provenance(key, provenance, resumable);
        self.relayout(key, layout).map(|_| ())
    }

    /// Moves a declared component to `domain`, each row to its entity's page there, values intact.
    pub fn move_component_domain(
        &mut self,
        key: ComponentKey,
        domain: SemanticDomain,
    ) -> Result<(), RegisterError> {
        let vtable = self
            .storage
            .registry
            .vtable(key)
            .ok_or(RegisterError::Unknown(key))?;
        let Some(layout) = vtable.columns.packed() else {
            return Err(RegisterError::NotRuntime {
                name: vtable.name.to_string(),
            });
        };
        let from = vtable.domain;
        if from == domain {
            return Ok(());
        }
        // Every row read out before any moves: detach then attach is the one
        // move a page knows how to make.
        let names: Vec<String> = layout.fields().iter().map(|f| f.name.clone()).collect();
        // Each row's tick too: a domain move is a move, not a change.
        let held: Vec<(EntityId, ScriptValue, Tick)> = self
            .iter_entities()
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|entity| {
                let row = self.row(entity, key)?;
                let fields = names
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, name)| Some((name.clone(), row.field(slot)?)))
                    .collect();
                let tick = self.carried_tick(entity, key);
                Some((entity, ScriptValue::Struct(fields), tick))
            })
            .collect();
        for (entity, _, _) in &held {
            let _ = self.detach(*entity, key);
        }
        self.storage.registry.set_domain(key, domain);
        // A plan names the domains its components live in.
        self.planner.query_cache.write_recovered().clear();
        for (entity, value, tick) in &held {
            if let Err(error) = self.add_runtime_component_at(*entity, key, value, *tick) {
                log::error!(
                    "moving a row of `{}` to {domain:?} lost it: {error:?}",
                    self.storage
                        .registry
                        .vtable(key)
                        .map_or("?", |vtable| &*vtable.name)
                );
            }
        }
        self.bump_domain_epoch(from);
        self.bump_domain_epoch(domain);
        Ok(())
    }

    /// Attaches a declared component with its defaults, then `value`'s fields.
    ///
    /// `value` is a `ScriptValue::Struct` or `Unit`. Nothing is attached when
    /// a field refuses its value.
    pub fn add_runtime_component(
        &mut self,
        entity: EntityId,
        key: ComponentKey,
        value: &ScriptValue,
    ) -> Result<(), RowError> {
        let now = self.clock.tick;
        self.add_runtime_component_at(entity, key, value, now)
    }

    /// Attaches as [`add_runtime_component`](Self::add_runtime_component) does,
    /// the row noted as written at `written_at`.
    pub(super) fn add_runtime_component_at(
        &mut self,
        entity: EntityId,
        key: ComponentKey,
        value: &ScriptValue,
        written_at: Tick,
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
                    if layout.accept(slot, value).is_some() {
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
        let attached = self.attach_at(
            entity,
            key,
            |column| {
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
            },
            written_at,
        );
        match attached {
            Ok(_) => Ok(()),
            Err(AddComponentError::ComponentAlreadyExists) => Err(RowError::AlreadyAttached),
            Err(AddComponentError::EntityNotFound) => Err(RowError::NoSuchEntity(entity)),
            Err(AddComponentError::ComponentNotRegistered) => Err(RowError::Unknown(key)),
        }
    }

    /// Detaches component `key`, Rust or declared, from `entity`; `false` when it held none.
    ///
    /// A behavior's state companion leaves with it, so a behavior attached
    /// again starts afresh.
    pub fn remove_component_by_key(&mut self, entity: EntityId, key: ComponentKey) -> bool {
        let companion = match self
            .storage
            .registry
            .vtable(key)
            .map(|vtable| &vtable.script)
        {
            Some(Some(role)) if role.runs().is_some() => self
                .storage
                .registry
                .vtable(key)
                .map(|vtable| format!("{}#state", vtable.name))
                .and_then(|name| self.storage.registry.key_named(&name)),
            _ => None,
        };
        let removed = self.detach(entity, key).is_ok();
        if removed {
            if let Some(companion) = companion {
                let _ = self.detach(entity, companion);
            }
        }
        removed
    }
}
