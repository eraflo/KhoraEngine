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

//! Saving and assigning materials.

use std::sync::{Arc, Mutex};

use khora_sdk::prelude::ecs::*;
use khora_sdk::{EditorState, GameWorld};

use crate::project_vfs::{AfterIndex, ProjectVfs};

/// Drains [`EditorState::pending_save_as_material`]: serializes the
/// entity's inline material to a `.kmat` (RON), writes it under
/// `assets/materials/<name>.kmat`, reindexes the VFS, then rewrites the
/// entity's component to `MaterialRef::Asset(uuid)` so it references the
/// freshly-saved shared, reloadable asset.
///
/// The `.kmat` bytes are exactly what the material decoder expects:
/// `material_to_json(&dyn Material)` (the `{ type_name, material }` value
/// split) RON-encoded. No project open → logged warning, no-op.
pub fn process_pending_save_as_material(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let pending = match editor_state.lock() {
        Ok(mut s) => s.pending_save_as_material.take(),
        Err(_) => None,
    };
    let Some((entity, name)) = pending else {
        return;
    };

    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Save material as .kmat ignored: no project is open");
        return;
    };

    // Serialize the entity's inline material to the decoder's RON shape.
    let ron_text = match inline_material_ron(world, entity) {
        Ok(text) => text,
        Err(why) => {
            log::warn!("Save material: {why}");
            return;
        }
    };

    let stem = sanitize_material_name(&name);
    let rel_fwd = format!("materials/{stem}.kmat");

    // Its identity, which a restore at Stop keeps: the assignment may land
    // after one. And the scene it was saved in: chosen how, edited or played.
    let persistent = world.inner_world().persistent_id(entity);
    let (choice, played) = editor_state.lock().map_or((0, false), |state| {
        (
            state.scene_choices,
            state.play_mode != khora_sdk::PlayMode::Editing,
        )
    });
    let assign_now = {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Save material: project VFS mutex poisoned");
            return;
        };
        if let Err(e) = pvfs.write_asset(std::path::Path::new(&rel_fwd), ron_text.as_bytes()) {
            log::error!("Save material: failed to write '{rel_fwd}': {e:#}");
            return;
        }
        // The identity comes from the registry: it needs no index.
        let uuid = pvfs.resolve_uuid(&rel_fwd);
        if let Err(e) = pvfs.reindex() {
            log::warn!("Save material: wrote '{rel_fwd}' but index rebuild failed: {e:#}");
        }
        // The entity references the asset once the index holds it, so no
        // decode of it is asked for before.
        pvfs.after_index(AfterIndex::AssignMaterial {
            entity,
            persistent,
            uuid,
            saved: ron_text.clone(),
            choice,
            played,
        })
    };
    log::info!(
        "Material saved to '{}' ({} bytes) for entity {:?}",
        rel_fwd,
        ron_text.len(),
        entity
    );
    if let Some(assign) = assign_now {
        apply_after_index(world, vec![assign]);
    }
}

/// The RON text of `entity`'s inline material, in the decoder's shape:
/// what a `.kmat` holds. Why not, when it has none or it cannot be encoded.
fn inline_material_ron(world: &GameWorld, entity: EntityId) -> Result<String, String> {
    let material = match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Inline { material, .. }) => material,
        Some(MaterialRef::Asset(_)) => {
            return Err(format!(
                "entity {entity:?} already references a .kmat asset"
            ))
        }
        None => return Err(format!("entity {entity:?} has no MaterialRef")),
    };
    let json = khora_sdk::khora_data::scene::material_to_json(&**material)
        .ok_or_else(|| "failed to serialize material to JSON".to_owned())?;
    ron::ser::to_string(&json).map_err(|e| format!("failed to encode material as RON: {e}"))
}

/// Applies the actions that waited for a reindex and need the world, as
/// [`crate::hot_reload::land_reindex`] returns them: an
/// [`AfterIndex::AssignMaterial`] replaces its entity's inline material with
/// a reference to the saved asset, if the entity is alive and still wears
/// the material saved.
pub fn apply_after_index(world: &mut GameWorld, actions: Vec<AfterIndex>) {
    for action in actions {
        let AfterIndex::AssignMaterial {
            entity,
            persistent,
            uuid,
            saved,
            ..
        } = action
        else {
            continue;
        };
        let entity = match persistent {
            Some(id) => match world.inner_world().entity_with_id(id) {
                Some(entity) => entity,
                None => continue,
            },
            None => entity,
        };
        // An entity despawned since, or whose material was given another
        // asset or edited since the save, is left as it is.
        if inline_material_ron(world, entity).ok().as_deref() != Some(saved.as_str()) {
            continue;
        }
        // `add_component` replaces the existing `MaterialRef` of the same
        // type: the entity now references the shared, reloadable asset.
        world.add_component(entity, MaterialRef::Asset(uuid));
        log::info!("Entity {entity:?} now references its saved material");
    }
}

/// Drains [`EditorState::pending_assign_material`]: sets
/// `MaterialRef::Asset(uuid)` on every selected entity, `uuid` being the
/// chosen material's identity.
pub fn process_pending_assign_material(
    _project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let (rel, targets) = match editor_state.lock() {
        Ok(mut s) => {
            let rel = s.pending_assign_material.take();
            let targets: Vec<EntityId> = s.selection.iter().copied().collect();
            (rel, targets)
        }
        Err(_) => return,
    };
    let Some(uuid) = rel else {
        return;
    };
    if targets.is_empty() {
        log::warn!("Assign material {uuid} ignored: no entity selected");
        return;
    }
    for entity in targets {
        world.add_component(entity, MaterialRef::Asset(uuid));
        log::info!("Assigned material {uuid} to entity {entity:?}");
    }
}

/// Strips characters unsafe in cross-platform file names from a material
/// name, falling back to `material` for an empty result.
fn sanitize_material_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_replacement = false;
    for ch in name.chars() {
        let safe = ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ');
        if safe {
            out.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            out.push('_');
            last_was_replacement = true;
        }
    }
    let trimmed = out.trim_matches(&[' ', '.', '_'][..]).to_string();
    if trimmed.is_empty() {
        "material".to_string()
    } else {
        trimmed
    }
}
