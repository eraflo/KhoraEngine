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
//! A scene is saved in its project only; one opened from outside the project
//! is read through a guard.

use crate::project_vfs::{AfterIndex, ProjectVfs};
#[cfg(test)]
use khora_sdk::khora_core::problems::ProblemSource;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};
use khora_sdk::khora_data::scene::loading::{SceneFetch, SceneSource};
use khora_sdk::khora_data::scene::record::ReportEntry;
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
/// A value only the session holds (a mesh made in code) is left out of the
/// file and logged as a warning.
pub fn save_scene_in_project(pvfs: &mut ProjectVfs, world: &GameWorld, rel_path: &Path) -> bool {
    save_scene_in_project_with_goal(pvfs, world, rel_path, SerializationGoal::EditorInterchange)
}

/// Saves as [`save_scene_in_project`] does, under an explicit serialization goal.
pub(crate) fn save_scene_in_project_with_goal(
    pvfs: &mut ProjectVfs,
    world: &GameWorld,
    rel_path: &Path,
    goal: SerializationGoal,
) -> bool {
    save_scene_in_project_reporting(pvfs, world, rel_path, goal).is_some()
}

/// Saves as [`save_scene_in_project_with_goal`] does, and returns what the
/// save left out of the file; `None` when the scene did not reach disk.
pub(crate) fn save_scene_in_project_reporting(
    pvfs: &mut ProjectVfs,
    world: &GameWorld,
    rel_path: &Path,
    goal: SerializationGoal,
) -> Option<Vec<ReportEntry>> {
    // Prefab instances are written as links to their prefabs, read through
    // the project's shared reader: no lock held while a prefab decodes, the
    // files as they are now.
    let service = SerializationService::with_prefabs(std::sync::Arc::new(
        khora_sdk::ReaderPrefabs::new(pvfs.assets().reader()),
    ));
    let saved = service.save_world_reporting(world.inner_world(), goal);
    let (scene_file, left_out) = match saved {
        Ok(saved) => saved,
        Err(e) => {
            log::error!("Failed to serialize scene: {:?}", e);
            return None;
        }
    };
    log_left_out(&rel_path.display().to_string(), &left_out);
    let bytes = scene_file.to_bytes();
    if let Err(e) = pvfs.write_asset(rel_path, &bytes) {
        log::error!("Failed to write scene to {:?}: {:#}", rel_path, e);
        return None;
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
    Some(left_out)
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
    // Registry-aware: a renamed scene keeps its frozen UUID.
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

/// The bytes of a scene file outside the project's `assets/`, read off the frame through a guard.
struct SceneOnDisk {
    /// What the file is read through: over the project's root, or over the file's folder.
    guard: khora_sdk::trust::FileGuard,
    /// The file, under the guard's root.
    rel: std::path::PathBuf,
}

impl SceneFetch for SceneOnDisk {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        self.guard
            .read(&self.rel)
            .map_err(|error| format!("{}: {error}", self.guard.root().join(&self.rel).display()))
    }
}

/// Returns where a load reads the project scene at `rel_path_fwd_slash` from,
/// and the prefabs it links to.
///
/// Its bytes are read off the frame; a scene missing from the index is
/// indexed again once. `None`, logged, when the project holds no such scene.
pub(crate) fn scene_in_project(
    pvfs: &mut ProjectVfs,
    rel_path_fwd_slash: &str,
    choice: u64,
) -> Option<(SceneSource, std::sync::Arc<dyn PrefabSource + Send + Sync>)> {
    // Registry-aware: a renamed scene keeps its frozen UUID.
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

/// Returns where a load reads the scene file at `path`, outside the project's `assets/`, from; it links to no prefabs.
///
/// A file under `project_root`, the open project's, is read through a guard
/// over that root at the project's trust; any other file through a guard over
/// its own folder, as a downloaded file, under the strict caps. Both report to
/// the refusals of `reads`, the run's.
pub fn scene_on_disk(
    path: &Path,
    reads: &crate::project_vfs::ProjectReads,
    project_root: Option<&Path>,
) -> (SceneSource, std::sync::Arc<dyn PrefabSource + Send + Sync>) {
    let in_project =
        project_root.and_then(|root| khora_sdk::fs::relative_to(path, root).map(|rel| (root, rel)));
    let fetch = match in_project {
        Some((root, rel)) => SceneOnDisk {
            guard: reads.guard(root),
            rel,
        },
        None => SceneOnDisk {
            guard: khora_sdk::trust::FileGuard::new(
                path.parent().unwrap_or(Path::new("")),
                khora_sdk::trust::Trust::Imported,
                reads.refusals(),
            ),
            rel: path
                .file_name()
                .map(std::path::PathBuf::from)
                .unwrap_or_default(),
        },
    };
    (
        SceneSource::Fetch(std::sync::Arc::new(fetch)),
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
    report_problems(world, &report.entries)
}

/// Returns report `entries`, a load's or a save's, as problems, one `Warning` per entry.
///
/// Each on the entity `world` holds under the entry's persistent id (none when
/// it holds none), its component and path as the field, the entry as the message.
pub fn report_problems(
    world: &khora_sdk::khora_data::ecs::World,
    entries: &[ReportEntry],
) -> Vec<Problem> {
    entries
        .iter()
        .map(|entry| Problem {
            entity: entry.entity.and_then(|id| world.entity_with_id(id)),
            component: entry.component.clone(),
            field: (!entry.path.is_empty()).then(|| entry.path.clone()),
            ..Problem::new(ProblemSeverity::Warning, entry.to_string())
        })
        .collect()
}

/// Spawns Main Camera + Directional Light entities, then saves the world at `rel_path` under the project's `assets/`; returns whether the save landed.
///
/// `rel_path` is forward-slash (default `scenes/default.kscene`); a failed
/// save is logged.
pub(crate) fn create_default_scene_in_project(
    pvfs: &mut ProjectVfs,
    world: &mut GameWorld,
    rel_path: &str,
) -> bool {
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

    let saved = save_scene_in_project(pvfs, world, Path::new(rel_path));
    if !saved {
        log::error!("Failed to seed default scene at '{}'", rel_path);
    }
    saved
}

/// Says what a save left out of its file — a mesh made in code — so a file
/// saved without it is never silent about it.
fn log_left_out(what: &str, left_out: &[ReportEntry]) {
    for entry in left_out {
        log::warn!("{what}: {entry}");
    }
}

#[cfg(test)]
mod tests;
