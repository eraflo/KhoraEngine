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

use super::asset_ops::resolve_asset_uuid;
use crate::project_vfs::ProjectVfs;

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
    let json = {
        let mref = match world.get_component::<MaterialRef>(entity) {
            Some(m) => m,
            None => {
                log::warn!("Save material: entity {:?} has no MaterialRef", entity);
                return;
            }
        };
        let material = match mref {
            MaterialRef::Inline { material, .. } => material,
            MaterialRef::Asset(_) => {
                log::warn!(
                    "Save material: entity {:?} already references a .kmat asset",
                    entity
                );
                return;
            }
        };
        match khora_sdk::khora_data::ecs::material_to_json(&**material) {
            Some(value) => value,
            None => {
                log::error!("Save material: failed to serialize material to JSON");
                return;
            }
        }
    };

    let ron_text = match ron::ser::to_string(&json) {
        Ok(text) => text,
        Err(e) => {
            log::error!("Save material: failed to encode material as RON: {e}");
            return;
        }
    };

    let stem = sanitize_material_name(&name);
    let rel_fwd = format!("materials/{stem}.kmat");

    let uuid = {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Save material: project VFS mutex poisoned");
            return;
        };
        if let Err(e) = pvfs.write_asset(std::path::Path::new(&rel_fwd), ron_text.as_bytes()) {
            log::error!("Save material: failed to write '{rel_fwd}': {e:#}");
            return;
        }
        if let Err(e) = pvfs.rebuild_index() {
            log::warn!("Save material: wrote '{rel_fwd}' but index rebuild failed: {e:#}");
        }
        pvfs.resolve_uuid(&rel_fwd)
    };

    // Convert the entity from an inline material to a reference to the
    // saved asset, so it is now shared and reloadable. `add_component`
    // replaces the existing `MaterialRef` of the same type.
    world.add_component(entity, MaterialRef::Asset(uuid));
    log::info!(
        "Material saved to '{}' ({} bytes); entity {:?} now references it",
        rel_fwd,
        ron_text.len(),
        entity
    );
}

/// Drains [`EditorState::pending_assign_material`]: sets
/// `MaterialRef::Asset(uuid)` on every selected entity, where `uuid` is
/// derived from the chosen `.kmat`'s forward-slash relative path.
pub fn process_pending_assign_material(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
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
    let Some(rel) = rel else {
        return;
    };
    if targets.is_empty() {
        log::warn!("Assign material '{rel}' ignored: no entity selected");
        return;
    }

    // Registry-aware so a renamed `.kmat` resolves to its frozen UUID.
    let uuid = resolve_asset_uuid(project_vfs, &rel);
    for entity in targets {
        world.add_component(entity, MaterialRef::Asset(uuid));
        log::info!("Assigned material '{rel}' to entity {entity:?}");
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
