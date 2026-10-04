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

//! Saving an entity as a prefab and spawning one.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{PrefabApplyScope, PropertyEdit};
use khora_sdk::khora_data::ecs::{PrefabInstance, World};
use khora_sdk::khora_data::scene::{
    apply_to_prefab, instance_of, instantiate_prefab, prefab_world, serialize_prefab,
    write_scene_file, CompactEncoding, InstanceOf, NoPrefabs, PrefabApply, PrefabSource,
};
use khora_sdk::prelude::ecs::*;
use khora_sdk::{serialize_subtree, EditorState, GameWorld};

use crate::ops::prefab_overrides::{json_overrides, rebound, reverted};

use crate::project_vfs::{ProjectPrefabs, ProjectVfs};
use crate::scene_io;

/// Drains [`EditorState::pending_save_as_prefab`] and
/// [`EditorState::pending_save_as_prefab_at`], writing the entity's
/// subtree as a `.kprefab` (a scene file of the subtree, compact encoding).
///
/// - The `_at` variant carries a pre-chosen forward-slash relative
///   path under `<project>/assets/`; the dispatcher writes directly
///   without showing a dialog. Set by drag-drop (entity → asset
///   browser folder).
/// - The plain variant opens an `rfd::FileDialog`. Set by the scene
///   tree's "Save as Prefab…" context entry.
pub fn process_pending_save_as_prefab(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    // Drag-drop path: pre-chosen folder, no dialog.
    let auto = match editor_state.lock() {
        Ok(mut s) => s.pending_save_as_prefab_at.take(),
        Err(_) => None,
    };
    if let Some((entity, rel_path)) = auto {
        save_prefab_to_project_path(project_vfs, world, entity, &rel_path);
    }

    // Right-click path: file dialog.
    let entity = match editor_state.lock() {
        Ok(mut s) => s.pending_save_as_prefab.take(),
        Err(_) => None,
    };
    let Some(entity) = entity else {
        return;
    };

    // The file first: the prefab it is decides what may be linked to.
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Khora Prefab", &["kprefab"])
        .set_file_name("prefab.kprefab")
        .save_file()
    else {
        return;
    };
    let path_str = path.to_string_lossy().to_string();

    if let Some(pvfs_arc) = project_vfs {
        let inside = match pvfs_arc.lock() {
            Ok(pvfs) => scene_io::rel_inside_project(&path, &pvfs.assets_root)
                .map(|rel| (pvfs.written_asset_id(&rel), rel)),
            Err(_) => {
                log::error!("Project VFS mutex poisoned");
                return;
            }
        };
        if let Some((written, rel_fwd)) = inside {
            let saved = serialize_prefab(
                world.inner_world(),
                entity,
                written,
                &ProjectPrefabs(pvfs_arc),
            );
            let bytes = match saved {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Failed to serialize prefab subtree: {:?}", e);
                    return;
                }
            };
            if let Ok(mut pvfs) = pvfs_arc.lock() {
                write_prefab_through_vfs(&mut pvfs, &rel_fwd, &bytes);
            }
            return;
        }
    }

    // Outside the project: no prefab of it can be the file written.
    let written = match project_vfs {
        Some(pvfs) => serialize_subtree(world.inner_world(), entity, &ProjectPrefabs(pvfs)),
        None => serialize_subtree(world.inner_world(), entity, &NoPrefabs),
    };
    let bytes = match written {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to serialize prefab subtree: {:?}", e);
            return;
        }
    };

    match std::fs::write(&path_str, &bytes) {
        Ok(()) => log::warn!(
            "Prefab saved to '{}' ({} bytes) — outside project, not VFS-managed.",
            path_str,
            bytes.len()
        ),
        Err(e) => log::error!("Failed to write prefab '{}': {}", path_str, e),
    }
}

fn save_prefab_to_project_path(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    entity: EntityId,
    rel_path: &str,
) {
    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Prefab drop ignored: no project is open");
        return;
    };
    // The prefab this file is: an instance of it inside it is written whole.
    let written = match pvfs_arc.lock() {
        Ok(pvfs) => pvfs.written_asset_id(rel_path),
        Err(_) => {
            log::error!("Project VFS mutex poisoned");
            return;
        }
    };
    let saved = serialize_prefab(
        world.inner_world(),
        entity,
        written,
        &ProjectPrefabs(pvfs_arc),
    );
    let bytes = match saved {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to serialize prefab subtree: {:?}", e);
            return;
        }
    };
    let Ok(mut pvfs) = pvfs_arc.lock() else {
        log::error!("Project VFS mutex poisoned");
        return;
    };
    write_prefab_through_vfs(&mut pvfs, rel_path, &bytes);
}

fn write_prefab_through_vfs(pvfs: &mut ProjectVfs, rel_fwd: &str, bytes: &[u8]) {
    if let Err(e) = pvfs.write_asset(std::path::Path::new(rel_fwd), bytes) {
        log::error!("Failed to write prefab '{}': {:#}", rel_fwd, e);
        return;
    }
    if let Err(e) = pvfs.rebuild_index() {
        log::warn!("Prefab saved but index rebuild failed: {:#}", e);
    }
    log::info!("Prefab saved to '{}' ({} bytes)", rel_fwd, bytes.len());
}

/// Drains [`EditorState::pending_prefab_spawn`] and brings the referenced
/// `.kprefab` into the live world as a linked instance via
/// [`instantiate_prefab`]. The forward-slash relative path resolves through
/// the project's VFS / `AssetService`.
pub fn process_pending_prefab_spawn(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let pending = match editor_state.lock() {
        Ok(mut s) => s.pending_prefab_spawn.take(),
        Err(_) => None,
    };
    let Some((rel, parent)) = pending else {
        return;
    };

    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Prefab spawn requested ('{}') but no project is open", rel);
        return;
    };

    let prefab = {
        let Ok(pvfs) = pvfs_arc.lock() else {
            log::error!("Project VFS mutex poisoned");
            return;
        };
        pvfs.resolve_uuid(&rel)
    };

    // Linked: the instance follows its prefab where it was not overridden.
    match instantiate_prefab(world.inner_world_mut(), prefab, &ProjectPrefabs(pvfs_arc)) {
        Ok(new_root) => {
            // Parent under the hierarchy row it was dropped on, if any.
            if let Some(parent) = parent {
                world.set_parent(new_root, Some(parent));
            }
            log::info!(
                "Prefab '{}' instantiated (root entity index={}{})",
                rel,
                new_root.index,
                if parent.is_some() { ", parented" } else { "" }
            );
        }
        Err(e) => log::error!("Failed to instantiate prefab '{}': {:?}", rel, e),
    }
}

/// Drains [`EditorState::pending_prefab_apply`]: what of the entity's
/// instance is applied is written into its prefab — the prefab's file
/// rewritten through the project's VFS and reindexed — so every instance of
/// it takes the value on its next load.
pub fn process_pending_prefab_apply(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let request = match editor_state.lock() {
        Ok(mut state) => state.pending_prefab_apply.take(),
        Err(_) => None,
    };
    let Some((entity, scope)) = request else {
        return;
    };
    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Apply to prefab ignored: no project is open");
        return;
    };
    let prefabs = ProjectPrefabs(pvfs_arc);
    let inner = world.inner_world();
    let Some(instance) = instance_of(inner, entity, &prefabs) else {
        log::warn!("Apply to prefab ignored: the entity is no part of a prefab instance");
        return;
    };
    let what = match scope {
        PrefabApplyScope::Field { type_name, path } => PrefabApply::Field {
            entity,
            component: type_name,
            path,
        },
        PrefabApplyScope::Component { type_name } => PrefabApply::Component {
            entity,
            component: type_name,
        },
        PrefabApplyScope::Instance => PrefabApply::Instance,
    };
    let record = match apply_to_prefab(inner, &instance, &what, &prefabs) {
        Ok(record) => record,
        Err(e) => {
            log::error!("Failed to apply to the prefab: {e}");
            return;
        }
    };
    let bytes = match write_scene_file(&record, &CompactEncoding) {
        Ok(file) => file.to_bytes(),
        Err(e) => {
            log::error!("Failed to write the prefab: {e}");
            return;
        }
    };

    // The other instances of the prefab in this world, as the prefab stood:
    // what they did not override follows the new prefab now, not on their
    // next load — a save in between would otherwise pin the old value as
    // an override.
    let others = other_instances(inner, &instance);
    let before: Vec<_> = others
        .iter()
        .filter_map(|other| {
            prefab_world(inner, other, &prefabs)
                .ok()
                .map(|w| (*other, w))
        })
        .collect();

    let rel = match pvfs_arc.lock() {
        Ok(pvfs) => pvfs.rel_path_of(instance.prefab),
        Err(_) => {
            log::error!("Project VFS mutex poisoned");
            return;
        }
    };
    let Some(rel) = rel else {
        log::error!("Apply to prefab: the prefab's file is not in the project index");
        return;
    };
    if let Ok(mut pvfs) = pvfs_arc.lock() {
        write_prefab_through_vfs(&mut pvfs, &rel, &bytes);
    }

    let edits = rebased_instances(inner, &before, &prefabs);
    if let Ok(mut state) = editor_state.lock() {
        for edit in edits {
            state.push_edit(edit);
        }
    }
}

/// Every instance of `instance`'s prefab in `world` but `instance` itself.
fn other_instances(world: &World, instance: &InstanceOf) -> Vec<InstanceOf> {
    world
        .query::<(EntityId, &PrefabInstance)>()
        .filter(|(root, link)| *root != instance.root && link.prefab == instance.prefab)
        .map(|(root, link)| InstanceOf {
            root,
            prefab: link.prefab,
        })
        .collect()
}

/// The edits that carry each instance in `before` onto its prefab as it now
/// is: a value the instance overrides stays the instance's; any other takes
/// the prefab's new one.
fn rebased_instances(
    world: &World,
    before: &[(InstanceOf, World)],
    prefabs: &dyn PrefabSource,
) -> Vec<PropertyEdit> {
    let mut edits = Vec::new();
    for (instance, old) in before {
        let Ok(new) = prefab_world(world, instance, prefabs) else {
            continue;
        };
        for twin in old.iter_entities() {
            let Some(id) = old.persistent_id(twin) else {
                continue;
            };
            let (Some(member), Some(now)) = (world.entity_with_id(id), new.entity_with_id(id))
            else {
                continue;
            };
            for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
                if !reg.is_saved() || reg.type_name == "PrefabInstance" {
                    continue;
                }
                let type_name = reg.type_name.to_owned();
                let live = (reg.to_json)(world, member);
                let was = (reg.to_json)(old, twin).map(|value| rebound(&value, old, world));
                let is = (reg.to_json)(&new, now).map(|value| rebound(&value, &new, world));
                match (live, was, is) {
                    // The new prefab value, with the instance's own overrides
                    // put back on top.
                    (Some(live), Some(was), Some(is)) => {
                        let rebased = json_overrides(&live, &was)
                            .iter()
                            .fold(is, |value, path| reverted(&value, &live, path));
                        if rebased != live {
                            edits.push(PropertyEdit::SetComponentJson {
                                entity: member,
                                type_name,
                                value: rebased,
                            });
                        }
                    }
                    // The prefab gained it: the instance, which had it not,
                    // gains it too.
                    (None, None, Some(is)) => edits.push(PropertyEdit::InsertComponentJson {
                        entity: member,
                        type_name,
                        value: is,
                    }),
                    // The prefab lost it: the instance loses it too, unless
                    // it had made it its own.
                    (Some(live), Some(was), None) if json_overrides(&live, &was).is_empty() => {
                        edits.push(PropertyEdit::RemoveComponent {
                            entity: member,
                            type_name,
                        })
                    }
                    _ => {}
                }
            }
        }
    }
    edits
}
