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

use khora_sdk::editor_ui::{AssetFileOp, EditorRequest};
use khora_sdk::khora_core::asset::{AssetType, AssetUUID};
use khora_sdk::prelude::ecs::*;
use khora_sdk::{AssetSource, EditorState, GameWorld};

use crate::project_vfs::ProjectVfs;
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
/// the identity registry, so references survive renames and moves. A script
/// written is then revealed.
pub fn process_pending_asset_file_ops(
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
        for op in ops {
            if let Some(rel) = run_file_op(&mut pvfs, op) {
                written_scripts.push(rel);
            }
        }
    }

    refresh_asset_cache(pvfs_arc, editor_state);
    if let Ok(mut s) = editor_state.lock() {
        for rel in written_scripts {
            s.push_request(EditorRequest::RevealAsset { path: rel });
        }
    }
}

/// Carries out one file operation, logging its outcome.
///
/// Returns the path of a script written that the index already lists, to reveal now.
fn run_file_op(pvfs: &mut ProjectVfs, op: AssetFileOp) -> Option<String> {
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
                    return pvfs
                        .after_index(crate::project_vfs::AfterIndex::Reveal(path))
                        .and_then(|now| match now {
                            crate::project_vfs::AfterIndex::Reveal(rel) => Some(rel),
                            _ => None,
                        });
                }
                Err(e) => log::error!("Create script '{path}' failed: {e:#}"),
            }
        }
        AssetFileOp::Rename { from, to } => match pvfs.rename_asset(&from, &to) {
            Ok(()) => log::info!("Renamed '{from}' → '{to}'"),
            Err(e) => log::error!("Rename '{from}' → '{to}' failed: {e:#}"),
        },
        AssetFileOp::Move { path, into } => match pvfs.move_asset(&path, &into) {
            Ok(()) => log::info!("Moved '{path}' → '{into}/'"),
            Err(e) => log::error!("Move '{path}' → '{into}' failed: {e:#}"),
        },
        AssetFileOp::Trash { path } => match pvfs.delete_to_trash(&path) {
            Ok(()) => log::info!("Moved '{path}' to the recycle bin"),
            Err(e) => log::error!("Delete '{path}' failed: {e:#}"),
        },
        AssetFileOp::Duplicate { path } => match pvfs.duplicate_asset(&path) {
            Ok(new_rel) => log::info!("Duplicated '{path}' → '{new_rel}'"),
            Err(e) => log::error!("Duplicate '{path}' failed: {e:#}"),
        },
    }
    None
}

/// Takes the queued [`EditorRequest::SpawnMesh`]s: spawns a `MeshRef::Asset` entity at each drop.
///
/// Asked by dragging a mesh tile onto the viewport or a hierarchy row; the
/// mesh loads on the next tick. The last one spawned is selected.
pub fn process_pending_spawn_mesh_asset(
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
    /// Its index type.
    asset_type: AssetType,
    /// The file's stem, followed by the part's label for a part of a file
    /// (`ship mesh1/prim0`).
    display_name: String,
}

/// `uuid` as the open project's index knows it, if it does.
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
        asset_type: metadata.asset_type.clone(),
        display_name,
    })
}

/// Takes the queued [`EditorRequest::AssignDropped`]s: assigns each dropped texture or `.kmat` to its entity.
///
/// A `.kmat` becomes a `MaterialRef::Asset`; an image becomes the
/// `base_color_texture` of a fresh inline standard material.
pub fn process_pending_assign_texture(
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
    match &asset.asset_type {
        AssetType::Material => {
            world.add_component(entity, MaterialRef::Asset(uuid));
            log::info!("Assigned material '{name}' to entity {entity:?}");
        }
        AssetType::Texture => {
            let std_mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color_texture: Some(uuid),
                ..Default::default()
            };
            world.add_component(entity, MaterialRef::inline(Box::new(std_mat)));
            log::info!("Assigned texture '{name}' to entity {entity:?} (base color)");
        }
        other => log::warn!(
            "Drop of '{name}' on entity {entity:?} ignored: a {other}, not a texture or material"
        ),
    }
}
