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

//! Projecting the components scripts read: a typed copy of each, read by
//! entity without the `World`.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, ProjectedComponent, ScriptValue,
};

use super::World;
use crate::ecs::ComponentKey;
use crate::scene::component_registration::registration_for;

impl World {
    /// A projection of the components named `names`: one typed copy of every
    /// page column holding each, and where each live holder's row is.
    ///
    /// A row a migration left behind is not a holder — only the row the
    /// entity's location points at is. A name no component has, or a
    /// component nobody holds, is projected empty, so whether a component is
    /// projected is a question with an answer.
    pub fn project_components(&self, names: &[ComponentName]) -> ComponentProjection {
        ComponentProjection::new(
            names
                .iter()
                .map(|name| self.project_component(name))
                .collect(),
        )
    }

    fn project_component(&self, name: &ComponentName) -> ProjectedComponent {
        let empty = || ProjectedComponent::new(name.clone(), Vec::new(), HashMap::new(), None);
        let registry = &self.storage.registry;
        let Some(key) = registry.key_named(name.as_str()) else {
            return empty();
        };
        let Some(vtable) = registry.vtable(key) else {
            return empty();
        };
        let mut columns: Vec<Box<dyn ColumnSnapshot>> = Vec::new();
        let mut index = HashMap::new();
        for (page_id, page) in self.storage.pages.iter().enumerate() {
            let Some(column) = page.columns.get(&key) else {
                continue;
            };
            let holders: Vec<(EntityId, usize)> = page
                .entities
                .iter()
                .enumerate()
                .filter(|&(row, &entity)| self.holds_at(entity, vtable.domain, page_id, row))
                .map(|(row, &entity)| (entity, row))
                .collect();
            if holders.is_empty() {
                continue;
            }
            let at = columns.len() as u32;
            columns.push(vtable.columns.snapshot(column.as_ref()));
            for (entity, row) in holders {
                index.insert(entity, (at, row as u32));
            }
        }
        ProjectedComponent::new(name.clone(), columns, index, default_row(self, key))
    }

    /// Every live row of declared component `key`: its entity and its fields
    /// by name, read from the pages that hold it and nothing else.
    pub fn declared_rows(&self, key: ComponentKey) -> Vec<(EntityId, Vec<(String, ScriptValue)>)> {
        let registry = &self.storage.registry;
        let Some(vtable) = registry.vtable(key) else {
            return Vec::new();
        };
        let Some(layout) = vtable.columns.packed() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (page_id, page) in self.storage.pages.iter().enumerate() {
            let Some(column) = page.columns.get(&key) else {
                continue;
            };
            for (row, &entity) in page.entities.iter().enumerate() {
                if !self.holds_at(entity, vtable.domain, page_id, row) {
                    continue;
                }
                let fields = layout
                    .fields()
                    .iter()
                    .enumerate()
                    .map(|(slot, field)| {
                        let value = vtable
                            .columns
                            .read_field(column.as_ref(), row, slot)
                            .unwrap_or_else(|| field.default.clone());
                        (field.name.clone(), value)
                    })
                    .collect();
                rows.push((entity, fields));
            }
        }
        rows
    }

    /// Every entity holding component `key` live, from the pages that hold
    /// it and nothing else — its values not read.
    pub fn holders(&self, key: ComponentKey) -> Vec<EntityId> {
        let Some(domain) = self
            .storage
            .registry
            .vtable(key)
            .map(|vtable| vtable.domain)
        else {
            return Vec::new();
        };
        let mut holders = Vec::new();
        for (page_id, page) in self.storage.pages.iter().enumerate() {
            if !page.columns.contains_key(&key) {
                continue;
            }
            holders.extend(
                page.entities
                    .iter()
                    .enumerate()
                    .filter(|&(row, &entity)| self.holds_at(entity, domain, page_id, row))
                    .map(|(_, &entity)| entity),
            );
        }
        holders
    }

    /// `entity`'s row of declared component `key`, its fields by name.
    pub fn declared_row(
        &self,
        entity: EntityId,
        key: ComponentKey,
    ) -> Option<Vec<(String, ScriptValue)>> {
        let row = self.row(entity, key)?;
        let layout = row.vtable().columns.packed()?;
        Some(
            layout
                .fields()
                .iter()
                .enumerate()
                .map(|(slot, field)| {
                    (
                        field.name.clone(),
                        row.field(slot).unwrap_or_else(|| field.default.clone()),
                    )
                })
                .collect(),
        )
    }

    /// Whether `row` of page `page_id` is `entity`'s live row in `domain`.
    fn holds_at(
        &self,
        entity: EntityId,
        domain: crate::ecs::SemanticDomain,
        page_id: usize,
        row: usize,
    ) -> bool {
        match self.entities.get(entity.index as usize) {
            Some((id, Some(metadata))) if *id == entity => {
                metadata.locations.get(&domain).is_some_and(|location| {
                    location.page_id as usize == page_id && location.row_index as usize == row
                })
            }
            _ => false,
        }
    }
}

/// Component `key`'s default, as a script reads it: a Rust component's from
/// its registration, a declared one's from its layout.
fn default_row(world: &World, key: ComponentKey) -> Option<khora_core::script::ScriptValue> {
    match key {
        ComponentKey::Rust(type_id) => {
            registration_for(type_id).and_then(|reg| (reg.script_default)())
        }
        ComponentKey::Declared(_) => {
            let layout = world.storage.registry.vtable(key)?.columns.packed()?;
            Some(khora_core::script::ScriptValue::Struct(
                layout
                    .fields()
                    .iter()
                    .map(|field| (field.name.clone(), field.default.clone()))
                    .collect(),
            ))
        }
    }
}
