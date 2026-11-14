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

//! Reparenting, duplicating and deleting entities.

use khora_sdk::editor_ui::*;
use khora_sdk::khora_data::scene::PrefabSource;
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

/// Drains `state.pending_reparent` and applies it to the ECS hierarchy.
///
/// Set by `scene_tree`'s drag-and-drop handler. The actual cycle check and
/// `Parent`/`Children` bookkeeping live in `GameWorld::set_parent`.
pub fn process_reparents(world: &mut GameWorld, state: &mut EditorState) {
    if let Some((child, new_parent)) = state.pending_reparent.take() {
        world.set_parent(child, new_parent);
        log::info!(
            "Reparented {:?} → {:?}",
            child,
            new_parent
                .map(|p| format!("{:?}", p))
                .unwrap_or_else(|| "<root>".to_owned())
        );
    }
}

/// Duplicates an entity and everything below it, as a sibling of the original.
///
/// Goes through the subtree round-trip "Save as Prefab" uses: the copy carries
/// every authored component, its references point inside the copy, and it
/// gets new identities. A prefab instance in the subtree is copied as an
/// instance of the same prefab, read from `prefabs`, with the same overrides.
pub fn duplicate_entity(
    world: &mut GameWorld,
    entity: EntityId,
    state: &mut EditorState,
    prefabs: &dyn PrefabSource,
) {
    let recipe = match khora_sdk::serialize_subtree(world.inner_world(), entity, prefabs) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("Duplicate failed: could not read {entity:?}: {e}");
            return;
        }
    };

    let original_parent = world.get_component::<Parent>(entity).map(|p: &Parent| p.0);
    let copy_name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| format!("{} (Copy)", n.as_str()))
        .unwrap_or_else(|| "Copy".to_owned());

    let new_entity = match khora_sdk::instantiate_subtree(world.inner_world_mut(), &recipe, prefabs)
    {
        Ok(id) => id,
        Err(e) => {
            log::error!("Duplicate failed: could not rebuild {entity:?}: {e}");
            return;
        }
    };

    if let Some(mut name) = world.get_component_mut::<Name>(new_entity) {
        *name = Name::new(copy_name);
    } else {
        world.add_component(new_entity, Name::new(copy_name));
    }
    if let Some(parent) = original_parent {
        world.set_parent(new_entity, Some(parent));
    }

    state.select(new_entity);
    log::info!("Duplicated entity {:?} -> {:?}", entity, new_entity);
}

/// Deletes every currently selected entity and clears selection/inspector state.
pub fn delete_selection(world: &mut GameWorld, state: &mut EditorState) {
    let to_delete: Vec<EntityId> = state.selection.iter().copied().collect();
    for entity in &to_delete {
        world.despawn(*entity);
    }
    if !to_delete.is_empty() {
        log::info!("Deleted {} entities", to_delete.len());
    }
    state.clear_selection();
    state.inspected = None;
}
