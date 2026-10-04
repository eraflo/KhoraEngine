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

//! The inspector's view of an entity, and applying its edits.

use super::scene_tree::domain_tag;
use khora_sdk::editor_ui::*;
use khora_sdk::khora_data::ecs::HierarchyWrite;
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

/// Extracts inspectable component snapshots for the single selected entity.
pub fn extract_inspected(world: &GameWorld, state: &mut EditorState) {
    let entity = match state.single_selected() {
        Some(entity) => entity,
        None => {
            state.inspected = None;
            return;
        }
    };
    // An entity is selected — drop any prior asset selection so the
    // Inspector switches out of asset-metadata mode.
    state.inspected_asset_path = None;

    let name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| n.as_str().to_owned())
        .unwrap_or_else(|| format!("Entity {}", entity.index));

    // Collect every component on this entity, captured generically as
    // JSON via the macro-generated `to_json`. The inspector walks this
    // list and renders every entry through a single field-typed walker
    // — adding a new ECS component costs zero editor code.
    //
    // We also populate the global `component_domain_registry` with the
    // domain of every registered type (regardless of whether this entity
    // has it) so the "+ Add Component" menu can categorise candidates
    // without re-querying the world.
    let inner_world = world.inner_world();
    let mut components_json = Vec::new();
    state.component_domain_registry.clear();
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        let domain = inner_world.component_domain(reg.type_id).map(domain_tag);
        if let Some(tag) = domain {
            state
                .component_domain_registry
                .insert(reg.type_name.to_string(), tag);
        }

        let Some(value) = (reg.to_json)(inner_world, entity) else {
            continue;
        };
        components_json.push(ComponentJson {
            type_name: reg.type_name.to_string(),
            domain,
            value,
        });
    }

    state.inspected = Some(InspectedEntity {
        entity,
        name,
        components_json,
        prefab: None,
    });
}

/// Applies queued property edits back into ECS components.
///
/// All component edits go through one path: the inspector ships a JSON
/// patch and we look up the component's `from_json` from the inventory
/// registration. The single special case is `Name`, which lives in the
/// inspector header (not as a component card) and so has its own variant.
pub fn apply_edits(world: &mut GameWorld, state: &mut EditorState) {
    let edits = state.drain_edits();
    for edit in edits {
        match edit {
            PropertyEdit::SetName(entity, new_name) => {
                if let Some(name) = world.get_component_mut::<Name>(entity) {
                    *name = Name::new(new_name);
                }
            }
            PropertyEdit::SetComponentJson {
                entity,
                type_name,
                value,
            } => {
                let inner = world.inner_world_mut();
                // The hierarchy is written by the module that owns it, so an
                // edited `Parent` moves both halves of the edge.
                if let Some(written) =
                    inner.write_hierarchy_by_name(entity, &type_name, HierarchyWrite::Set(&value))
                {
                    if let Err(e) = written {
                        log::warn!("Failed to apply JSON edit to {}: {}", type_name, e);
                    }
                    continue;
                }
                let mut applied = false;
                for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
                    if reg.type_name == type_name {
                        match (reg.from_json)(inner, entity, &value) {
                            Ok(()) => applied = true,
                            Err(e) => {
                                log::warn!("Failed to apply JSON edit to {}: {}", type_name, e)
                            }
                        }
                        break;
                    }
                }
                if !applied {
                    log::warn!("No registration found for component '{}'", type_name);
                }
            }
            PropertyEdit::RemoveComponent { entity, type_name } => {
                let inner = world.inner_world_mut();
                if let Some(written) =
                    inner.write_hierarchy_by_name(entity, &type_name, HierarchyWrite::Remove)
                {
                    if let Err(e) = written {
                        log::warn!("Failed to remove component {}: {}", type_name, e);
                    }
                    continue;
                }
                let mut applied = false;
                for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
                    if reg.type_name == type_name {
                        match (reg.remove)(inner, entity) {
                            Ok(()) => applied = true,
                            Err(e) => log::warn!("Failed to remove component {}: {}", type_name, e),
                        }
                        break;
                    }
                }
                if !applied {
                    log::warn!("No registration found for component '{}'", type_name);
                }
            }
            PropertyEdit::InsertComponentJson {
                entity,
                type_name,
                value,
            } => {
                // The component with this value: added first where the entity
                // lacks it, then given the value — a revert of a removed one.
                // The hierarchy is written by the module that owns it, both
                // halves of the edge at once.
                let inner = world.inner_world_mut();
                if let Some(written) =
                    inner.write_hierarchy_by_name(entity, &type_name, HierarchyWrite::Set(&value))
                {
                    if let Err(e) = written {
                        log::warn!("Failed to apply JSON edit to {}: {}", type_name, e);
                    }
                    continue;
                }
                let Some(reg) = inventory::iter::<khora_sdk::ComponentRegistration>
                    .into_iter()
                    .find(|reg| reg.type_name == type_name)
                else {
                    log::warn!("No registration found for component '{}'", type_name);
                    continue;
                };
                if (reg.to_json)(inner, entity).is_none() {
                    if let Err(e) = (reg.create_default)(inner, entity) {
                        log::warn!("Failed to add component {}: {}", type_name, e);
                        continue;
                    }
                }
                if let Err(e) = (reg.from_json)(inner, entity, &value) {
                    log::warn!("Failed to apply JSON edit to {}: {}", type_name, e);
                }
            }
        }
    }
}

/// Adds a new component to an existing entity by dispatching through the inventory registry.
pub fn add_component_to_entity(world: &mut GameWorld, entity: EntityId, type_name: &str) {
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        if reg.type_name == type_name {
            if let Err(e) = (reg.create_default)(world.inner_world_mut(), entity) {
                log::error!("Failed to add component {}: {}", type_name, e);
            } else {
                log::info!("Added {} component to entity {:?}", type_name, entity);
            }
            return;
        }
    }
    log::warn!("No component registration found for type '{}'", type_name);
}
