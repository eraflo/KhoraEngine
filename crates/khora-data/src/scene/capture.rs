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

//! Writing a world, or part of one, down as a scene record — page by page.
//!
//! The page is the unit CRPECS stores and serializes in, so capture walks the
//! pages: for each, the rows that are live (a page may hold rows an entity
//! has left, waiting for compaction), and for each component worth saving, its
//! column read row by row. The values are written by name through the
//! component's registration; how the column is laid out in memory never
//! reaches the record.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::component_registration::ComponentRegistration;
use super::record::{EntityRef, RecordError, ReferenceWriter};
use super::scene_record::{PageRecord, SceneRecord};
use crate::ecs::{Children, Parent, World};

/// Why a world could not be written down.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// A component's value could not be written as a record.
    Component {
        /// The component's type name.
        component: String,
        /// Why.
        error: RecordError,
    },
    /// The root of a subtree is not alive.
    NoSuchEntity(EntityId),
    /// The record could not be encoded.
    Encoding(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Component { component, error } => write!(f, "`{component}`: {error}"),
            Self::NoSuchEntity(entity) => write!(f, "no entity {entity:?}"),
            Self::Encoding(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Every entity of `world` and the components worth saving.
///
/// Listed tree by tree, each parent's children in their `Children` order: a
/// load derives every list from the order the record names its children in,
/// so this is what keeps siblings in the order the scene had them.
pub fn capture_world(world: &World) -> Result<SceneRecord, SaveError> {
    let roots = world
        .iter_entities()
        .filter(|entity| {
            world
                .get::<Parent>(*entity)
                .is_none_or(|parent| !world.contains(parent.0))
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut entities = descend(world, roots, &mut seen);
    // An entity no root reaches sits on a loop a damaged hierarchy made; it
    // is still saved, after the trees.
    entities.extend(world.iter_entities().filter(|entity| seen.insert(*entity)));
    capture(world, &entities, None)
}

/// `root` and everything under it. A reference to an entity outside the
/// subtree is written as outside, and the root's own parent is left out: the
/// subtree is placed wherever it is instantiated.
pub fn capture_subtree(world: &World, root: EntityId) -> Result<SceneRecord, SaveError> {
    if !world.contains(root) {
        return Err(SaveError::NoSuchEntity(root));
    }
    let entities = descend(world, vec![root], &mut HashSet::new());
    capture(world, &entities, Some(root))
}

/// `roots` and everything under them, breadth first, each parent's children
/// in their `Children` order. Each entity once, however the lists name it: a
/// list that repeats a child, or names an ancestor, does not make the walk
/// loop.
fn descend(world: &World, roots: Vec<EntityId>, seen: &mut HashSet<EntityId>) -> Vec<EntityId> {
    let mut entities: Vec<EntityId> = roots
        .into_iter()
        .filter(|root| seen.insert(*root))
        .collect();
    let mut next = 0;
    while next < entities.len() {
        if let Some(children) = world.get::<Children>(entities[next]) {
            for &child in &children.0 {
                if world.contains(child) && seen.insert(child) {
                    entities.push(child);
                }
            }
        }
        next += 1;
    }
    entities
}

/// The registrations of the components worth saving — those an author or a
/// tool wrote, not those the engine derives or writes while running — by type.
fn saved_registrations() -> HashMap<TypeId, &'static ComponentRegistration> {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .filter(|reg| reg.is_saved())
        .map(|reg| (reg.type_id, reg))
        .collect()
}

fn capture(
    world: &World,
    entities: &[EntityId],
    root: Option<EntityId>,
) -> Result<SceneRecord, SaveError> {
    let mut inside = InsideOnly {
        ids: HashMap::with_capacity(entities.len()),
    };
    let mut ids = Vec::with_capacity(entities.len());
    for &entity in entities {
        let id = world
            .persistent_id(entity)
            .ok_or(SaveError::NoSuchEntity(entity))?;
        inside.ids.insert(entity, id);
        ids.push(id);
    }
    let captured: HashSet<EntityId> = entities.iter().copied().collect();

    let registrations = saved_registrations();
    let parent = TypeId::of::<crate::ecs::Parent>();
    // Pages whose saved components are the same land in one page record:
    // the record holds what a load needs to build, one page per signature.
    let mut pages: Vec<PageRecord> = Vec::new();
    let mut by_signature: HashMap<Vec<&'static str>, usize> = HashMap::new();

    for (page_id, page) in world.storage.pages.iter().enumerate() {
        let mut saved: Vec<&'static ComponentRegistration> = page
            .type_ids
            .iter()
            .filter_map(|type_id| registrations.get(type_id).copied())
            .collect();
        if saved.is_empty() {
            continue;
        }
        saved.sort_by_key(|reg| reg.type_name);

        for (row, &entity) in page.entities.iter().enumerate() {
            if !captured.contains(&entity) || !world.is_live_row_of(page_id as u32, row, entity) {
                continue;
            }
            // The subtree root's parent is outside the subtree: left out, so
            // the instance is placed wherever it is made.
            let columns: Vec<&&ComponentRegistration> = saved
                .iter()
                .filter(|reg| !(Some(entity) == root && reg.type_id == parent))
                .collect();
            if columns.is_empty() {
                continue;
            }
            let signature: Vec<&'static str> = columns.iter().map(|reg| reg.type_name).collect();
            let index = *by_signature.entry(signature.clone()).or_insert_with(|| {
                pages.push(PageRecord {
                    components: signature.iter().map(|name| (*name).to_owned()).collect(),
                    rows: Vec::new(),
                    columns: vec![Vec::new(); signature.len()],
                });
                pages.len() - 1
            });
            let record = &mut pages[index];
            record.rows.push(inside.ids[&entity]);
            for (slot, reg) in columns.iter().enumerate() {
                // In the signature, so in the columns: a page keeps both in step.
                let Some(column) = page.columns.get(&reg.type_id) else {
                    return Err(SaveError::Component {
                        component: reg.type_name.to_owned(),
                        error: RecordError("its page has no column for it".to_owned()),
                    });
                };
                let value =
                    (reg.column_to_record)(column.as_ref(), row, &mut inside).map_err(|error| {
                        SaveError::Component {
                            component: reg.type_name.to_owned(),
                            error,
                        }
                    })?;
                record.columns[slot].push(value);
            }
        }
    }

    Ok(SceneRecord {
        entities: ids,
        pages,
    })
}

/// Writes an entity the record holds by its identity, any other as outside.
struct InsideOnly {
    ids: HashMap<EntityId, PersistentId>,
}

impl ReferenceWriter for InsideOnly {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.ids
            .get(&entity)
            .map_or(EntityRef::Outside, |id| EntityRef::Id(*id))
    }
}
