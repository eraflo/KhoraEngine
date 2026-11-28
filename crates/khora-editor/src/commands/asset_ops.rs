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

//! File operations on assets, and assigning or spawning them.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetFileOp, EditorRequest, SceneInProject};
use khora_sdk::khora_core::asset::{kinds, AssetKind, AssetUUID, InlineMaterial, Material};
use khora_sdk::khora_data::ecs::AssetRef;
use khora_sdk::prelude::ecs::*;
use khora_sdk::{AssetSource, EditorState, GameWorld};

use crate::project_vfs::{AfterIndex, KeyChange, ProjectVfs};
use crate::{hot_reload, ops};

/// Recomputes the asset-browser cache (entries + directories) after a file
/// operation and bumps the epoch so the panel rescans its flattened view.
fn refresh_asset_cache(pvfs_arc: &Arc<Mutex<ProjectVfs>>, editor_state: &Arc<Mutex<EditorState>>) {
    let refreshed = match pvfs_arc.lock() {
        Ok(pvfs) => Some((hot_reload::collect_asset_entries(&pvfs), pvfs.list_dirs())),
        Err(_) => None,
    };
    if let Some((entries, dirs)) = refreshed {
        if let Ok(mut s) = editor_state.lock() {
            s.asset_entries = entries;
            s.asset_dirs = dirs;
            s.asset_epoch = s.asset_epoch.wrapping_add(1);
        }
    }
}

/// Takes the asset browser's queued file operations: new folder or script, rename, move, trash, duplicate.
///
/// They run in the order asked. Each goes through [`ProjectVfs`], which keeps
/// the identity registry, so references survive renames and moves; what the
/// editor names by path follows each rename, move and trash
/// ([`follow_key_changes`]). A script written is then revealed.
pub fn run_queued_asset_file_ops(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let ops = match editor_state.lock() {
        Ok(mut s) => s.take_requests(|request| match request {
            EditorRequest::AssetFile(op) => Ok(op),
            other => Err(other),
        }),
        Err(_) => return,
    };
    if ops.is_empty() {
        return;
    }
    let Some(pvfs_arc) = project_vfs else {
        log::warn!("Asset file operation ignored: no project is open");
        return;
    };

    let mut written_scripts = Vec::new();
    {
        let Ok(mut pvfs) = pvfs_arc.lock() else {
            log::error!("Asset file op: project VFS mutex poisoned");
            return;
        };
        let mut changes = Vec::new();
        for op in ops {
            match run_file_op(&mut pvfs, op) {
                Ran::Wrote(rel) => written_scripts.push(rel),
                Ran::Changed(change) => changes.push(change),
                Ran::Other => {}
            }
        }
        // The project before the state, as every other holder of both locks.
        if let Ok(mut state) = editor_state.lock() {
            follow_key_changes(&mut pvfs, &mut state, &changes);
        }
    }

    refresh_asset_cache(pvfs_arc, editor_state);
    if let Ok(mut s) = editor_state.lock() {
        for rel in written_scripts {
            s.push_request(EditorRequest::RevealAsset { path: rel });
        }
    }
}

/// Makes everything the editor names by an asset key follow `changes`, in order.
///
/// A key moved is named by its new key, one trashed is forgotten: the scene a
/// save writes to (trashed, the scene has none, so a save asks where as Save
/// As does), the scenes still opening, the asset inspected, the requests
/// queued that name a path, and what waits for a reindex in `pvfs`
/// ([`ProjectVfs::follow_key_change`]). The holders of a key outside `state`,
/// the asset browser's shown folder, follow them from
/// `EditorState::key_changes`, where each is added. The prefab view and the
/// asset picker read the index again once its generation moves.
pub(crate) fn follow_key_changes(
    pvfs: &mut ProjectVfs,
    state: &mut EditorState,
    changes: &[KeyChange],
) {
    state.key_changes.extend_from_slice(changes);
    for change in changes {
        pvfs.follow_key_change(change);
        let assets = pvfs.assets_root.clone();
        let follow_scene = |scene: SceneInProject| {
            if !scene.is_in(&assets) {
                return Some(scene);
            }
            (change.follow(scene.key())).map(|key| SceneInProject::new(scene.assets(), key))
        };
        state.current_scene_path = state.current_scene_path.take().and_then(follow_scene);
        for opening in &mut state.opening {
            opening.scene = opening.scene.take().and_then(follow_scene);
        }
        if let Some(key) = state.inspected_asset_path.take() {
            state.inspected_asset_path = change.follow(&key);
            if state.inspected_asset_path.is_none() {
                state.selected_asset = None;
            }
        }
        let queued = std::mem::take(&mut state.requests);
        state.requests = queued
            .into_iter()
            .filter_map(|request| followed_request(request, change))
            .collect();
    }
}

/// Returns `request` as it reads once `change` is made; `None` when it named a path `change` trashed.
fn followed_request(request: EditorRequest, change: &KeyChange) -> Option<EditorRequest> {
    match request {
        EditorRequest::RevealAsset { path } => change
            .follow(&path)
            .map(|path| EditorRequest::RevealAsset { path }),
        EditorRequest::SaveAsPrefabAt { entity, path } => {
            // A folder's path ends in `/`: the prefab takes the entity's name.
            let (key, slash) = match path.strip_suffix('/') {
                Some(folder) => (folder, "/"),
                None => (path.as_str(), ""),
            };
            match change.follow(key) {
                Some(key) => Some(EditorRequest::SaveAsPrefabAt {
                    entity,
                    path: format!("{key}{slash}"),
                }),
                None => {
                    log::warn!("The prefab is not saved: '{path}' went to the recycle bin");
                    None
                }
            }
        }
        other => Some(other),
    }
}

/// What one file operation did, for the editor to follow.
enum Ran {
    /// It wrote a script the index already lists, to reveal now.
    Wrote(String),
    /// It moved or trashed what the editor may name by its key.
    Changed(KeyChange),
    /// Nothing the editor follows: a folder or a copy made, or a failure.
    Other,
}

/// Carries out one file operation, logging its outcome, and returns what the editor follows of it.
fn run_file_op(pvfs: &mut ProjectVfs, op: AssetFileOp) -> Ran {
    match op {
        AssetFileOp::CreateFolder { path } => match pvfs.create_folder(&path) {
            Ok(()) => log::info!("Created folder '{path}'"),
            Err(e) => log::error!("Create folder '{path}' failed: {e:#}"),
        },
        AssetFileOp::CreateScript { path, source } => {
            match pvfs.create_asset(&path, source.as_bytes()) {
                Ok(()) => {
                    log::info!("Created script '{path}'");
                    // Revealed once the index lists it: at once when the
                    // project was indexed on the frame.
                    let now = pvfs.after_index(AfterIndex::Reveal(path));
                    if let Some(AfterIndex::Reveal(rel)) = now {
                        return Ran::Wrote(rel);
                    }
                }
                Err(e) => log::error!("Create script '{path}' failed: {e:#}"),
            }
        }
        AssetFileOp::Rename { from, to } => match pvfs.rename_asset(&from, &to) {
            Ok(change) => {
                log::info!("Renamed '{from}' → '{to}'");
                return Ran::Changed(change);
            }
            Err(e) => log::error!("Rename '{from}' → '{to}' failed: {e:#}"),
        },
        AssetFileOp::Move { path, into } => match pvfs.move_asset(&path, &into) {
            Ok(change) => {
                log::info!("Moved '{path}' → '{into}/'");
                return Ran::Changed(change);
            }
            Err(e) => log::error!("Move '{path}' → '{into}' failed: {e:#}"),
        },
        AssetFileOp::Trash { path } => match pvfs.delete_to_trash(&path) {
            Ok(change) => {
                log::info!("Moved '{path}' to the recycle bin");
                return Ran::Changed(change);
            }
            Err(e) => log::error!("Delete '{path}' failed: {e:#}"),
        },
        AssetFileOp::Duplicate { path } => match pvfs.duplicate_asset(&path) {
            Ok(new_rel) => log::info!("Duplicated '{path}' → '{new_rel}'"),
            Err(e) => log::error!("Duplicate '{path}' failed: {e:#}"),
        },
    }
    Ran::Other
}

/// Takes the queued [`EditorRequest::SpawnMesh`]s: spawns an `AssetRef::Asset` entity at each drop.
///
/// Asked by dragging a mesh tile onto the viewport or a hierarchy row; the
/// mesh loads on the next tick. The last one spawned is selected.
pub fn spawn_queued_meshes(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let spawns = editor_state
        .lock()
        .map(|mut s| {
            s.take_requests(|request| match request {
                EditorRequest::SpawnMesh {
                    mesh,
                    point,
                    parent,
                } => Ok((mesh, point, parent)),
                other => Err(other),
            })
        })
        .unwrap_or_default();
    for (uuid, point, parent) in spawns {
        let name = indexed_asset(project_vfs, uuid)
            .map_or_else(|| "Mesh".to_owned(), |asset| asset.display_name);
        let entity = ops::spawn_mesh_asset(world, uuid, point, &name);
        // Parent under the hierarchy row it was dropped on, if any.
        if let Some(parent) = parent {
            world.set_parent(entity, Some(parent));
        }
        if let Ok(mut s) = editor_state.lock() {
            s.select(entity);
        }
        log::info!("Spawned mesh '{name}' as entity {entity:?} at {point:?}");
    }
}

/// An asset of the open project, as the editor names and dispatches it.
struct IndexedAsset {
    /// Its kind.
    kind: AssetKind,
    /// The file's stem, followed by the part's label for a part of a file
    /// (`ship mesh1/prim0`).
    display_name: String,
}

/// Returns `uuid` as the open project's index knows it, if it does.
fn indexed_asset(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    uuid: AssetUUID,
) -> Option<IndexedAsset> {
    let pvfs = project_vfs?.lock().ok()?;
    let assets = pvfs.assets();
    let metadata = assets.index().get_metadata(&uuid)?;
    let stem = metadata
        .source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let display_name = match metadata.default_source() {
        Some(AssetSource::Sub { label, .. }) => format!("{stem} {label}"),
        _ => stem,
    };
    Some(IndexedAsset {
        kind: metadata.kind,
        display_name,
    })
}

/// Takes the queued [`EditorRequest::AssignDropped`]s: assigns each dropped texture or `.kmat` to its entity.
///
/// A `.kmat` becomes an `AssetRef::Asset`; an image becomes the
/// `base_color_texture` of a fresh inline standard material.
pub fn assign_queued_drops(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
) {
    let drops = editor_state
        .lock()
        .map(|mut s| {
            s.take_requests(|request| match request {
                EditorRequest::AssignDropped { asset, entity } => Ok((asset, entity)),
                other => Err(other),
            })
        })
        .unwrap_or_default();
    for (uuid, entity) in drops {
        assign_dropped(project_vfs, world, uuid, entity);
    }
}

/// Gives `entity` the texture or material `uuid` names.
fn assign_dropped(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    world: &mut GameWorld,
    uuid: AssetUUID,
    entity: EntityId,
) {
    let Some(asset) = indexed_asset(project_vfs, uuid) else {
        log::warn!(
            "Drop of asset {uuid} on entity {entity:?} ignored: the project has no such asset"
        );
        return;
    };
    let name = &asset.display_name;
    match asset.kind {
        kinds::MATERIAL => {
            world.add_component(entity, AssetRef::<Box<dyn Material>>::Asset(uuid));
            log::info!("Assigned material '{name}' to entity {entity:?}");
        }
        kinds::TEXTURE => {
            let std_mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color_texture: Some(uuid),
                ..Default::default()
            };
            world.add_component(
                entity,
                AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(std_mat))),
            );
            log::info!("Assigned texture '{name}' to entity {entity:?} (base color)");
        }
        other => log::warn!(
            "Drop of '{name}' on entity {entity:?} ignored: a {other}, not a texture or material"
        ),
    }
}
