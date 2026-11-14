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

//! Scene serialization helpers: saves and loads through the project's VFS.
//!
//! A Save As or an Open outside the project falls back to `std::fs`.

use crate::project_vfs::{AfterIndex, ProjectVfs};
use khora_sdk::khora_core::asset::asset_key;
#[cfg(test)]
use khora_sdk::khora_core::problems::ProblemSource;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};
use khora_sdk::khora_data::scene::loading::{SceneFetch, SceneSource};
use khora_sdk::khora_data::scene::{NoPrefabs, PrefabSource};
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{LinearRgba, Vec3};
#[cfg(test)]
use khora_sdk::SceneFile;
#[cfg(test)]
use khora_sdk::DEFAULT_SCENE_REL_PATH as DEFAULT_SCENE_REL;
use khora_sdk::{GameWorld, SerializationGoal, SerializationService};
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────────
// Play-mode snapshot. See the book: concepts/serialization.md, "Play-mode snapshots".
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes the whole world in memory, for Stop to restore.
///
/// Returns an empty `Vec` on failure: Stop then leaves the live state in
/// place, better than a panic mid-play.
pub fn snapshot_scene(world: &GameWorld) -> Vec<u8> {
    let svc = SerializationService::new();
    match svc.save_world(world.inner_world(), SerializationGoal::FastestLoad) {
        Ok(scene) => scene.to_bytes(),
        Err(e) => {
            log::error!("Play-mode snapshot failed: {:?}", e);
            Vec::new()
        }
    }
}

/// Says what a load adapted — a field defaulted, dropped, renamed — so an
/// older scene loading into newer code is never silent about it.
pub(crate) fn log_report(what: &str, report: &khora_sdk::khora_data::scene::record::LoadReport) {
    for entry in &report.entries {
        log::warn!("{what}: {entry}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Project-relative saves and loads — primary path used by Save / Open / auto-load
// ─────────────────────────────────────────────────────────────────────────────

/// Saves the world as a `.kscene` at `rel_path` under the project's `assets/`, then reindexes.
///
/// Under the `EditorInterchange` goal; [`save_scene_in_project_with_goal`]
/// picks another (`HumanReadableDebug` for JSON, `PortableBinary` for MessagePack).
pub fn save_scene_in_project(pvfs: &mut ProjectVfs, world: &GameWorld, rel_path: &Path) -> bool {
    save_scene_in_project_with_goal(pvfs, world, rel_path, SerializationGoal::EditorInterchange)
}

/// Saves as [`save_scene_in_project`] does, under an explicit serialization goal.
pub fn save_scene_in_project_with_goal(
    pvfs: &mut ProjectVfs,
    world: &GameWorld,
    rel_path: &Path,
    goal: SerializationGoal,
) -> bool {
    // Prefab instances are written as links to their prefabs.
    let service = SerializationService::new();
    // Read through the project's shared reader: no lock held while a prefab
    // decodes, the files as they are now.
    let prefabs = khora_sdk::ReaderPrefabs::new(pvfs.assets().reader());
    let saved = service.save_world_with(world.inner_world(), goal, &prefabs);
    let scene_file = match saved {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to serialize scene: {:?}", e);
            return false;
        }
    };
    let bytes = scene_file.to_bytes();
    if let Err(e) = pvfs.write_asset(rel_path, &bytes) {
        log::error!("Failed to write scene to {:?}: {:#}", rel_path, e);
        return false;
    }
    if let Err(e) = pvfs.reindex() {
        log::warn!("Scene saved but index rebuild failed: {:#}", e);
    }
    log::info!(
        "Scene saved to '{}' ({} bytes, goal={:?}) via ProjectVfs",
        rel_path.display(),
        bytes.len(),
        goal,
    );
    true
}

// Synchronous: for tests and scratch worlds; the editor opens a scene by ticket.
#[cfg(test)]
/// Loads a scene by its path under `<project>/assets/`, reindexing once when
/// the VFS does not know it yet (just saved).
#[cfg(test)]
pub fn load_scene_in_project(
    pvfs: &mut ProjectVfs,
    world: &mut GameWorld,
    rel_path_fwd_slash: &str,
) -> bool {
    load_scene_in_project_with_report(pvfs, world, rel_path_fwd_slash).is_some()
}

// Synchronous: for tests and scratch worlds; the editor opens a scene by ticket.
#[cfg(test)]
/// Loads as [`load_scene_in_project`] does, returning what the load adapted.
///
/// The [`LoadReport`] it also logs; `None` when the scene did not load.
///
/// [`LoadReport`]: khora_sdk::khora_data::scene::record::LoadReport
pub fn load_scene_in_project_with_report(
    pvfs: &mut ProjectVfs,
    world: &mut GameWorld,
    rel_path_fwd_slash: &str,
) -> Option<khora_sdk::khora_data::scene::record::LoadReport> {
    // Registry-aware: a scene that was renamed keeps its frozen UUID.
    let uuid = pvfs.resolve_uuid(rel_path_fwd_slash);

    // Try the existing index first; if absent, reindex once and retry.
    let indexed = pvfs.assets().load_raw(&uuid);
    let bytes = match indexed {
        Ok(b) => b,
        Err(_) => {
            if let Err(e) = pvfs.rebuild_index() {
                log::error!(
                    "Failed to rebuild index while resolving '{}': {:#}",
                    rel_path_fwd_slash,
                    e
                );
                return None;
            }
            let reindexed = pvfs.assets().load_raw(&uuid);
            match reindexed {
                Ok(b) => b,
                Err(e) => {
                    log::error!(
                        "Failed to load scene '{}' from ProjectVfs: {:#}",
                        rel_path_fwd_slash,
                        e
                    );
                    return None;
                }
            }
        }
    };

    let scene_file = match SceneFile::from_bytes(&bytes) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Invalid scene file '{}': {:?}", rel_path_fwd_slash, e);
            return None;
        }
    };

    // Prefab instances are expanded from their prefabs as they are now.
    let service = SerializationService::new();
    let prefabs = khora_sdk::ReaderPrefabs::new(pvfs.assets().reader());
    let loaded = service.replace_world_with(&scene_file, world.inner_world_mut(), &prefabs);
    match loaded {
        Ok(report) => {
            log_report(rel_path_fwd_slash, &report);
            log::info!(
                "Scene loaded from '{}' ({} bytes) via ProjectVfs",
                rel_path_fwd_slash,
                bytes.len()
            );
            Some(report)
        }
        Err(e) => {
            log::error!(
                "Failed to deserialize scene '{}': {:?}",
                rel_path_fwd_slash,
                e
            );
            None
        }
    }
}

/// The bytes of a project asset, read off the frame through the project's
/// asset reader.
struct ProjectScene {
    reader: std::sync::Arc<khora_sdk::AssetReader>,
    uuid: khora_sdk::prelude::AssetUUID,
}

impl SceneFetch for ProjectScene {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        self.reader
            .read_raw(&self.uuid)
            .map_err(|error| format!("{error:#}"))
    }
}

/// The bytes of a scene file outside the project, read off the frame.
struct SceneOnDisk(std::path::PathBuf);

impl SceneFetch for SceneOnDisk {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| format!("{}: {error}", self.0.display()))
    }
}

/// Returns where a load reads the project scene at `rel_path_fwd_slash` from,
/// and the prefabs it links to.
///
/// Its bytes are read off the frame; a scene missing from the index is
/// indexed again once. `None`, logged, when the project holds no such scene.
pub fn scene_in_project(
    pvfs: &mut ProjectVfs,
    rel_path_fwd_slash: &str,
    choice: u64,
) -> Option<(SceneSource, std::sync::Arc<dyn PrefabSource + Send + Sync>)> {
    // Registry-aware: a scene that was renamed keeps its frozen UUID.
    let uuid = pvfs.resolve_uuid(rel_path_fwd_slash);
    if pvfs.assets().reader().index().get_metadata(&uuid).is_none() {
        if let Err(e) = pvfs.reindex() {
            log::error!(
                "Failed to rebuild index while resolving '{}': {:#}",
                rel_path_fwd_slash,
                e
            );
            return None;
        }
        // Indexed off the frame, the scene opens once the index holds it;
        // indexed on the frame (no engine), it is looked up again now.
        let open = AfterIndex::OpenScene {
            rel: rel_path_fwd_slash.to_owned(),
            choice,
        };
        if pvfs.after_index(open).is_none() {
            log::info!("Indexing the project before opening '{rel_path_fwd_slash}'");
            return None;
        }
        if pvfs.assets().reader().index().get_metadata(&uuid).is_none() {
            log::error!("The project holds no scene '{}'", rel_path_fwd_slash);
            return None;
        }
    }
    let reader = pvfs.assets().reader();
    let prefabs = std::sync::Arc::new(khora_sdk::ReaderPrefabs::new(std::sync::Arc::clone(
        &reader,
    )));
    Some((
        SceneSource::Fetch(std::sync::Arc::new(ProjectScene { reader, uuid })),
        prefabs,
    ))
}

/// Returns where a load reads the scene file at `path`, outside the project,
/// from; it links to no prefabs.
pub fn scene_on_disk(path: &Path) -> (SceneSource, std::sync::Arc<dyn PrefabSource + Send + Sync>) {
    (
        SceneSource::Fetch(std::sync::Arc::new(SceneOnDisk(path.to_path_buf()))),
        std::sync::Arc::new(NoPrefabs),
    )
}

// Synchronous: for tests and scratch worlds; the editor opens a scene by ticket.
#[cfg(test)]
/// Puts what a load adapted in `problems`, as the `Load` source's warnings.
///
/// Each on the entity `world` holds under its persistent id, its component and
/// path as the field. Replaces what the last load reported; a clean report clears it.
pub fn report_load(
    problems: &khora_sdk::khora_core::problems::Problems,
    world: &khora_sdk::khora_data::ecs::World,
    report: &khora_sdk::khora_data::scene::record::LoadReport,
) {
    problems.report(ProblemSource::Load, load_problems(world, report));
}

/// Returns what a load adapted as problems, one `Warning` per entry.
///
/// On the entity `world` holds under the entry's persistent id (none when it
/// holds none), its component and path as the field, the entry as the message.
pub fn load_problems(
    world: &khora_sdk::khora_data::ecs::World,
    report: &khora_sdk::khora_data::scene::record::LoadReport,
) -> Vec<Problem> {
    report
        .entries
        .iter()
        .map(|entry| Problem {
            severity: ProblemSeverity::Warning,
            module: None,
            line: 0,
            column: 0,
            message: entry.to_string(),
            note: None,
            entity: entry.entity.and_then(|id| world.entity_with_id(id)),
            behavior: None,
            component: entry.component.clone(),
            field: (!entry.path.is_empty()).then(|| entry.path.clone()),
        })
        .collect()
}

/// Spawns Main Camera + Directional Light entities, then saves the world to
/// the relative path inside the project (default `scenes/default.kscene`).
pub(crate) fn create_default_scene_in_project(
    pvfs: &mut ProjectVfs,
    world: &mut GameWorld,
    rel_path: &str,
) {
    let camera = world.spawn((
        Transform {
            translation: Vec3::new(0.0, 5.0, 10.0),
            ..Default::default()
        },
        GlobalTransform::identity(),
        Camera::default(),
        Name("Main Camera".to_string()),
    ));

    let light = world.spawn((
        Transform {
            translation: Vec3::new(0.0, 10.0, 0.0),
            ..Default::default()
        },
        GlobalTransform::identity(),
        Light::new(LightType::Directional(DirectionalLight {
            direction: Vec3::new(-0.4, -0.8, -0.45),
            color: LinearRgba::WHITE,
            intensity: 1.0,
            ..Default::default()
        })),
        Name("Directional Light".to_string()),
    ));
    for entity in [camera, light] {
        world.inner_world_mut().mark_authored(entity);
    }

    if !save_scene_in_project(pvfs, world, Path::new(rel_path)) {
        log::error!("Failed to seed default scene at '{}'", rel_path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Arbitrary-path fallback — for File→Save As / Open dialogs that target a
// location outside the current project. Bypasses the VFS by design.
// ─────────────────────────────────────────────────────────────────────────────

/// Saves the world as a `.kscene` at an absolute path outside `<project>/assets/`.
///
/// For a Save As outside the project; logs a warning, the write bypassing the VFS.
pub fn save_scene_to_path_with_goal(
    world: &GameWorld,
    path: &str,
    goal: SerializationGoal,
) -> bool {
    let service = SerializationService::new();
    match service.save_world(world.inner_world(), goal) {
        Ok(scene_file) => {
            let bytes = scene_file.to_bytes();
            match std::fs::write(path, &bytes) {
                Ok(()) => {
                    log::warn!(
                        "Scene saved to '{}' ({} bytes, goal={:?}) — outside project, not VFS-managed.",
                        path,
                        bytes.len(),
                        goal,
                    );
                    true
                }
                Err(e) => {
                    log::error!("Failed to write scene file '{}': {}", path, e);
                    false
                }
            }
        }
        Err(e) => {
            log::error!("Failed to serialize scene: {:?}", e);
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Path utilities
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `abs_path` relative to `assets_root`, forward-slashed, if it lies under it.
///
/// `None` otherwise: callers read the scene from its path ([`scene_on_disk`]).
pub fn rel_inside_project(abs_path: &Path, assets_root: &Path) -> Option<String> {
    let rel = abs_path.strip_prefix(assets_root).ok()?;
    Some(asset_key(rel))
}

#[cfg(test)]
mod tests;
