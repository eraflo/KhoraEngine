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

//! Scene serialization helpers.
//!
//! All scene I/O is **project-relative when possible** — saves and loads route
//! through [`crate::project_vfs::ProjectVfs`] so the editor uses the same
//! `AssetService` + `FileLoader` contract as a future runtime. The "Save As" /
//! "Open" file-dialog flows still accept arbitrary paths (that's intentional —
//! you might want to open a scene from outside the current project) and fall
//! back to direct `std::fs` only in that case.

use crate::project_vfs::ProjectVfs;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{LinearRgba, Vec3};
use khora_sdk::{GameWorld, SceneFile, SerializationGoal, SerializationService};
use std::path::Path;

/// Canonical relative path of the auto-created default scene.
pub const DEFAULT_SCENE_REL: &str = "scenes/default.kscene";

// ─────────────────────────────────────────────────────────────────────────────
// Play-mode snapshot — full-world capture via SerializationService.
//
// Routed through `SerializationService::FastestLoad` (the Archetype strategy)
// because the snapshot is throw-away in-memory bytes: write/read latency
// dominates, file size and human-readability don't matter. The previous
// hand-rolled binary format only captured `Transform` and silently dropped
// every other component during the play→stop cycle.
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes the entire world to an in-memory byte buffer for play-mode
/// restore. Returns an empty `Vec` on failure (caller logs and proceeds —
/// the worst case is "Stop button leaves the live state in place", which is
/// preferable to a panic mid-play).
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

/// Restores the world from a snapshot produced by [`snapshot_scene`].
///
/// Despawns every existing entity before deserializing — the live world after
/// gameplay may have spawned new entities or destroyed old ones, so we
/// rebuild from the snapshot rather than diff against it.
pub fn restore_scene(world: &mut GameWorld, snapshot: &[u8]) {
    if snapshot.is_empty() {
        return;
    }
    let scene = match SceneFile::from_bytes(snapshot) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Play-mode restore: invalid snapshot: {:?}", e);
            return;
        }
    };

    let all: Vec<_> = world.iter_entities().collect();
    for e in all {
        world.despawn(e);
    }

    let svc = SerializationService::new();
    if let Err(e) = svc.load_world(&scene, world.inner_world_mut()) {
        log::error!("Play-mode restore failed: {:?}", e);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Project-relative saves and loads — primary path used by Save / Open / auto-load
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes the current world to a `.kscene` file at `rel_path` (relative
/// to the project's `assets/` root) via the project's `AssetService`. Re-
/// indexes on success so the new scene is immediately resolvable by UUID.
///
/// Default callers should use [`save_scene_in_project`] which selects
/// `EditorInterchange` (Recipe / bincode). Use
/// [`save_scene_in_project_with_goal`] to pick a different strategy
/// (`HumanReadableDebug` for RON export, `FastestLoad` for archetype).
#[allow(dead_code)]
pub fn save_scene_in_project(pvfs: &mut ProjectVfs, world: &GameWorld, rel_path: &Path) -> bool {
    save_scene_in_project_with_goal(pvfs, world, rel_path, SerializationGoal::EditorInterchange)
}

/// Same as [`save_scene_in_project`] with an explicit serialization goal.
pub fn save_scene_in_project_with_goal(
    pvfs: &mut ProjectVfs,
    world: &GameWorld,
    rel_path: &Path,
    goal: SerializationGoal,
) -> bool {
    let agent = SerializationService::new();
    let scene_file = match agent.save_world(world.inner_world(), goal) {
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
    if let Err(e) = pvfs.rebuild_index() {
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

/// Loads a scene by its relative path under `<project>/assets/` via the
/// project's `AssetService::load_raw`. Falls back to a fresh reindex + retry
/// once if the path isn't yet known to the VFS (e.g. just-saved).
pub fn load_scene_in_project(
    pvfs: &mut ProjectVfs,
    world: &mut GameWorld,
    rel_path_fwd_slash: &str,
) -> bool {
    let uuid = ProjectVfs::uuid_for_rel_path(rel_path_fwd_slash);

    // Try the existing index first; if absent, reindex once and retry.
    let bytes = match pvfs.asset_service.load_raw(&uuid) {
        Ok(b) => b,
        Err(_) => {
            if let Err(e) = pvfs.rebuild_index() {
                log::error!(
                    "Failed to rebuild index while resolving '{}': {:#}",
                    rel_path_fwd_slash,
                    e
                );
                return false;
            }
            match pvfs.asset_service.load_raw(&uuid) {
                Ok(b) => b,
                Err(e) => {
                    log::error!(
                        "Failed to load scene '{}' from ProjectVfs: {:#}",
                        rel_path_fwd_slash,
                        e
                    );
                    return false;
                }
            }
        }
    };

    let scene_file = match SceneFile::from_bytes(&bytes) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Invalid scene file '{}': {:?}", rel_path_fwd_slash, e);
            return false;
        }
    };

    // Despawn current world before deserializing.
    let all_entities: Vec<_> = world.iter_entities().collect();
    for entity in all_entities {
        world.despawn(entity);
    }

    let agent = SerializationService::new();
    match agent.load_world(&scene_file, world.inner_world_mut()) {
        Ok(()) => {
            log::info!(
                "Scene loaded from '{}' ({} bytes) via ProjectVfs",
                rel_path_fwd_slash,
                bytes.len()
            );
            true
        }
        Err(e) => {
            log::error!(
                "Failed to deserialize scene '{}': {:?}",
                rel_path_fwd_slash,
                e
            );
            false
        }
    }
}

/// Auto-loads the project's default scene (`assets/scenes/default.kscene`),
/// creating it from a Camera + Light template if it doesn't exist yet.
pub fn auto_load_or_create_default_scene(pvfs: &mut ProjectVfs, world: &mut GameWorld) {
    let rel = DEFAULT_SCENE_REL;
    let abs = pvfs.assets_root.join(Path::new(rel));
    if abs.exists() {
        load_scene_in_project(pvfs, world, rel);
    } else {
        create_default_scene_in_project(pvfs, world, rel);
    }
}

/// Spawns Main Camera + Directional Light entities, then saves the world to
/// the relative path inside the project (default `scenes/default.kscene`).
fn create_default_scene_in_project(pvfs: &mut ProjectVfs, world: &mut GameWorld, rel_path: &str) {
    world.spawn((
        Transform {
            translation: Vec3::new(0.0, 5.0, 10.0),
            ..Default::default()
        },
        GlobalTransform::identity(),
        Camera::default(),
        Name("Main Camera".to_string()),
    ));

    world.spawn((
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

    if !save_scene_in_project(pvfs, world, Path::new(rel_path)) {
        log::error!("Failed to seed default scene at '{}'", rel_path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Arbitrary-path fallback — for File→Save As / Open dialogs that target a
// location outside the current project. Bypasses the VFS by design.
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes the world to a `.kscene` at an absolute path. Used by the
/// "Save As..." dialog when the user picks a destination outside
/// `<project>/assets/`. Logs a warning so the divergence from VFS-managed
/// I/O is visible.
#[allow(dead_code)]
pub fn save_scene_to_path(world: &GameWorld, path: &str) {
    save_scene_to_path_with_goal(world, path, SerializationGoal::EditorInterchange)
}

/// Same as [`save_scene_to_path`] with an explicit serialization goal.
pub fn save_scene_to_path_with_goal(world: &GameWorld, path: &str, goal: SerializationGoal) {
    let agent = SerializationService::new();
    match agent.save_world(world.inner_world(), goal) {
        Ok(scene_file) => {
            let bytes = scene_file.to_bytes();
            match std::fs::write(path, &bytes) {
                Ok(()) => log::warn!(
                    "Scene saved to '{}' ({} bytes, goal={:?}) — outside project, not VFS-managed.",
                    path,
                    bytes.len(),
                    goal,
                ),
                Err(e) => log::error!("Failed to write scene file '{}': {}", path, e),
            }
        }
        Err(e) => log::error!("Failed to serialize scene: {:?}", e),
    }
}

/// Loads a scene from an absolute path. Used by the "Open..." dialog when
/// the user picks a file outside `<project>/assets/`.
pub fn load_scene_from_path(world: &mut GameWorld, path: &str) -> bool {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("Failed to read scene file '{}': {}", path, e);
            return false;
        }
    };

    let scene_file = match SceneFile::from_bytes(&bytes) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Invalid scene file '{}': {:?}", path, e);
            return false;
        }
    };

    let all_entities: Vec<_> = world.iter_entities().collect();
    for entity in all_entities {
        world.despawn(entity);
    }

    let agent = SerializationService::new();
    match agent.load_world(&scene_file, world.inner_world_mut()) {
        Ok(()) => {
            log::warn!(
                "Scene loaded from '{}' ({} bytes) — outside project, not VFS-managed.",
                path,
                bytes.len()
            );
            true
        }
        Err(e) => {
            log::error!("Failed to deserialize scene '{}': {:?}", path, e);
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Path utilities
// ─────────────────────────────────────────────────────────────────────────────

/// If `abs_path` lives under `<project>/assets/`, returns the relative path
/// in forward-slash form ready for [`load_scene_in_project`]. Otherwise
/// returns `None` — callers should fall back to [`load_scene_from_path`].
pub fn rel_inside_project(abs_path: &Path, assets_root: &Path) -> Option<String> {
    let rel = abs_path.strip_prefix(assets_root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}
