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

//! Pure ECS operations used by the editor application.

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

mod hierarchy;
mod inspect;
mod scene_tree;
mod spawn;

pub use hierarchy::delete_selection;
pub use hierarchy::duplicate_entity;
pub use hierarchy::process_reparents;
pub use inspect::add_component_to_entity;
pub use inspect::apply_edits;
pub use inspect::extract_inspected;
pub use scene_tree::extract_scene_tree;
pub use spawn::process_spawns;
pub use spawn::spawn_mesh_asset;

/// Keeps ECS scene-camera activation consistent with the current play mode.
pub fn sync_scene_cameras_for_mode(world: &mut GameWorld, mode: PlayMode) {
    let entities: Vec<EntityId> = world.iter_entities().collect();
    let camera_states: Vec<(EntityId, bool)> = entities
        .iter()
        .filter_map(|&entity| {
            world
                .get_component::<Camera>(entity)
                .map(|cam| (entity, cam.is_active))
        })
        .collect();

    match mode {
        // Editing mode always uses the dedicated editor camera.
        PlayMode::Editing => {
            for (entity, _) in camera_states {
                if let Some(cam) = world.get_component_mut::<Camera>(entity) {
                    cam.is_active = false;
                }
            }
        }
        // During play/pause, ensure at least one scene camera is active.
        PlayMode::Playing | PlayMode::Paused => {
            if camera_states.iter().any(|(_, is_active)| *is_active) {
                return;
            }
            if let Some((entity, _)) = camera_states.first().copied() {
                if let Some(cam) = world.get_component_mut::<Camera>(entity) {
                    cam.is_active = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
