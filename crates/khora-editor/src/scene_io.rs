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
use khora_sdk::khora_core::asset::asset_key;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{LinearRgba, Vec3};
use khora_sdk::{GameWorld, SceneFile, SerializationGoal, SerializationService};
use std::path::Path;

// Canonical relative path of the auto-created default scene: the one the
// runtime loads when `runtime.json` names none.
use khora_sdk::DEFAULT_SCENE_REL_PATH as DEFAULT_SCENE_REL;

// ─────────────────────────────────────────────────────────────────────────────
// Play-mode snapshot — full-world capture via SerializationService as a
// snapshot (`FastestLoad`): the same build writes and reads it, in memory, so
// the fastest form is the right one. Stop restores it atomically and with the
// same persistent identities, so references held by scripts land on the
// restored entities.
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
/// Replaces the whole world — gameplay may have spawned or destroyed
/// entities, so it is rebuilt from the snapshot rather than diffed against
/// it — and does so atomically: a snapshot that cannot be read leaves the
/// live state in place.
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

    let svc = SerializationService::new();
    match svc.replace_world(&scene, world.inner_world_mut()) {
        Ok(report) => log_report("play-mode restore", &report),
        Err(e) => log::error!("Play-mode restore failed: {:?}", e),
    }
}

/// Says what a load adapted — a field defaulted, dropped, renamed — so an
/// older scene loading into newer code is never silent about it.
fn log_report(what: &str, report: &khora_sdk::khora_data::scene::record::LoadReport) {
    for entry in &report.entries {
        log::warn!("{what}: {entry}");
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
/// `EditorInterchange` (the compact encoding). Use
/// [`save_scene_in_project_with_goal`] to pick another encoding
/// (`HumanReadableDebug` for readable JSON, `PortableBinary` for MessagePack).
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
    let service = SerializationService::new();
    let scene_file = match service.save_world(world.inner_world(), goal) {
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
    // Registry-aware: a scene that was renamed keeps its frozen UUID.
    let uuid = pvfs.resolve_uuid(rel_path_fwd_slash);

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

    let service = SerializationService::new();
    match service.replace_world(&scene_file, world.inner_world_mut()) {
        Ok(report) => {
            log_report(rel_path_fwd_slash, &report);
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

/// Serializes the world to a `.kscene` at an absolute path. Used by the
/// "Save As..." dialog when the user picks a destination outside
/// `<project>/assets/`. Logs a warning so the divergence from VFS-managed
/// I/O is visible.
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

    let service = SerializationService::new();
    match service.replace_world(&scene_file, world.inner_world_mut()) {
        Ok(report) => {
            log_report(path, &report);
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
    Some(asset_key(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: snapshot/restore must round-trip the `Name` component.
    /// Symptom of a broken implementation: entities show "Entity N" in the
    /// scene tree after Stop because the fallback in `extract_scene_tree`
    /// kicks in.
    #[test]
    fn snapshot_restore_preserves_name() {
        let mut world = GameWorld::new();
        world.spawn((
            Transform {
                translation: Vec3::new(1.0, 2.0, 3.0),
                ..Default::default()
            },
            GlobalTransform::identity(),
            Name::new("TestCube"),
        ));

        let snap = snapshot_scene(&world);
        assert!(!snap.is_empty(), "snapshot should not be empty");

        // Despawn everything to simulate gameplay.
        let all: Vec<_> = world.iter_entities().collect();
        for e in all {
            world.despawn(e);
        }
        assert_eq!(world.iter_entities().count(), 0);

        restore_scene(&mut world, &snap);

        let names: Vec<String> = world
            .iter_entities()
            .filter_map(|e| {
                world
                    .get_component::<Name>(e)
                    .map(|n| n.as_str().to_owned())
            })
            .collect();
        assert!(
            names.contains(&"TestCube".to_string()),
            "expected 'TestCube' Name to survive restore, got {:?}",
            names
        );
    }

    /// Save/load round-trip beyond the single-`Name` case: several entities,
    /// each carrying multiple components with non-default field values, must
    /// survive snapshot → restore with entity count and key field values
    /// intact. Covers Transform + Name + Light + Camera.
    #[test]
    fn snapshot_restore_preserves_multiple_components() {
        let mut world = GameWorld::new();

        let cube = world.spawn((
            Transform::from_translation(Vec3::new(4.0, 5.0, 6.0)),
            GlobalTransform::identity(),
            Name::new("Cube"),
        ));
        let _light = world.spawn((
            Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)),
            GlobalTransform::identity(),
            Name::new("Sun"),
            Light::directional(),
        ));
        let _camera = world.spawn((
            Transform::from_translation(Vec3::new(0.0, 2.0, 9.0)),
            GlobalTransform::identity(),
            Name::new("Cam"),
            Camera::new_perspective(std::f32::consts::FRAC_PI_4, 1.5, 0.1, 800.0),
        ));
        let _ = cube;

        let before = world.iter_entities().count();
        assert_eq!(before, 3);

        let snap = snapshot_scene(&world);
        assert!(!snap.is_empty());

        // Throw the live world away and rebuild from the snapshot.
        let all: Vec<_> = world.iter_entities().collect();
        for e in all {
            world.despawn(e);
        }
        restore_scene(&mut world, &snap);

        assert_eq!(
            world.iter_entities().count(),
            before,
            "entity count must survive the round-trip"
        );

        // The cube's translation and name must come back exactly.
        let restored: Vec<_> = world.iter_entities().collect();
        let cube_back = restored
            .iter()
            .find(|&&e| {
                world
                    .get_component::<Name>(e)
                    .is_some_and(|n| n.as_str() == "Cube")
            })
            .copied()
            .expect("the 'Cube' entity must survive restore");
        let t = world
            .get_component::<Transform>(cube_back)
            .expect("restored cube keeps its Transform");
        assert_eq!(t.translation, Vec3::new(4.0, 5.0, 6.0));

        // The Light and Camera components must come back on their entities.
        let has_light = restored
            .iter()
            .any(|&e| world.get_component::<Light>(e).is_some());
        let has_camera = restored
            .iter()
            .any(|&e| world.get_component::<Camera>(e).is_some());
        assert!(has_light, "a Light must survive the round-trip");
        assert!(has_camera, "a Camera must survive the round-trip");
    }

    /// Play/stop guard: entities that share storage pages across domains
    /// (mesh entities carry Spatial + Render components) are snapshotted,
    /// despawned wholesale during "play", then restored. The pre-play state
    /// must come back fully — this exercises the multi-domain despawn path
    /// that previously corrupted survivor rows.
    #[test]
    fn play_stop_restores_state_after_multi_domain_despawn() {
        let mut world = GameWorld::new();

        // Two mesh entities (Spatial Transform + Render MeshRef) plus a light,
        // so the snapshot spans pages in more than one semantic domain.
        let a = world.spawn((
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            GlobalTransform::identity(),
            Name::new("MeshA"),
            MeshRef::procedural(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0]),
        ));
        let _b = world.spawn((
            Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
            GlobalTransform::identity(),
            Name::new("MeshB"),
            MeshRef::procedural(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]),
        ));
        let _light = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            Name::new("Light"),
            Light::point(),
        ));
        let _ = a;

        let before = world.iter_entities().count();
        assert_eq!(before, 3);

        // Enter play: snapshot the authoring state.
        let snap = snapshot_scene(&world);
        assert!(!snap.is_empty());

        // During play, the simulation despawns everything (and could spawn more).
        let all: Vec<_> = world.iter_entities().collect();
        for e in all {
            world.despawn(e);
        }
        world.spawn((Transform::identity(), GlobalTransform::identity()));
        assert_ne!(world.iter_entities().count(), before);

        // Stop: restore the pre-play state.
        restore_scene(&mut world, &snap);

        assert_eq!(
            world.iter_entities().count(),
            before,
            "stop must restore the exact pre-play entity count"
        );

        // A survivor's data across both domains must be intact.
        let restored: Vec<_> = world.iter_entities().collect();
        let mesh_a = restored
            .iter()
            .find(|&&e| {
                world
                    .get_component::<Name>(e)
                    .is_some_and(|n| n.as_str() == "MeshA")
            })
            .copied()
            .expect("'MeshA' must survive restore");
        let t = world
            .get_component::<Transform>(mesh_a)
            .expect("restored MeshA keeps its Spatial Transform");
        assert_eq!(t.translation, Vec3::new(1.0, 0.0, 0.0));
        assert!(
            world.get_component::<MeshRef>(mesh_a).is_some(),
            "restored MeshA keeps its Render-domain MeshRef"
        );
    }

    /// Stop rebuilds the scene from the play snapshot, so every entity comes
    /// back as a new `EntityId`. A script that pointed at another entity —
    /// in an authored field, or in a register of a sequence frozen at an
    /// `await` — must point at that entity's restored self after Stop, found
    /// by its persistent identity, not at the slot it occupied before play.
    #[test]
    fn stop_restores_script_targets() {
        use khora_sdk::khora_core::script::{
            FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
            SuspendedMachine,
        };

        let mut world = GameWorld::new();
        let target = world.spawn((
            Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
            GlobalTransform::identity(),
            Name::new("Target"),
        ));
        let guard = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            Name::new("Guard"),
            Script {
                module: "ai/guard.erg".into(),
                behavior: "Guard".into(),
                fields: vec![("target".into(), ScriptValue::Entity(target))],
                runtime: ScriptSnapshot {
                    pending: Some(PendingSequence {
                        fingerprint: 7,
                        remaining: 0.5,
                        machine: SuspendedMachine::Frozen(FrozenMachine {
                            body: PendingBody::Update,
                            registers: vec![FrozenValue::Entity(target)],
                            frames: vec![FrozenFrame {
                                function: "on_update".into(),
                                base: 0,
                                return_pc: 0,
                                result: 0,
                            }],
                            program_counter: 3,
                        }),
                    }),
                    ..ScriptSnapshot::default()
                },
            },
        ));
        // The editor marks authored every entity it creates.
        let target_id = world
            .inner_world_mut()
            .mark_authored(target)
            .expect("authored");
        world
            .inner_world_mut()
            .mark_authored(guard)
            .expect("authored");

        // Play: snapshot, then the game rearranges the world — the target's
        // slot is freed and reused by something else.
        let snap = snapshot_scene(&world);
        assert!(!snap.is_empty());
        world.despawn(target);
        world.despawn(guard);
        let usurper = world.spawn((Transform::identity(), Name::new("Usurper")));
        world.spawn((Transform::identity(), Name::new("Bystander")));

        // Stop.
        restore_scene(&mut world, &snap);

        let named = |world: &GameWorld, name: &str| {
            world
                .iter_entities()
                .find(|&e| {
                    world
                        .get_component::<Name>(e)
                        .is_some_and(|n| n.as_str() == name)
                })
                .unwrap_or_else(|| panic!("'{name}' must exist after Stop"))
        };
        let target_back = named(&world, "Target");
        let guard_back = named(&world, "Guard");
        assert!(
            world.iter_entities().all(|e| world
                .get_component::<Name>(e)
                .is_none_or(|n| n.as_str() != "Usurper")),
            "Stop removes what play spawned"
        );
        let _ = usurper;
        assert_eq!(
            world.inner_world().persistent_id(target_back),
            Some(target_id),
            "the target keeps its identity across play"
        );

        let script = world
            .get_component::<Script>(guard_back)
            .expect("the guard keeps its script");
        assert_eq!(
            script.field("target"),
            Some(&ScriptValue::Entity(target_back)),
            "the authored field points at the restored target"
        );
        let pending = script
            .runtime
            .pending
            .as_ref()
            .expect("the pending sequence");
        let SuspendedMachine::Frozen(machine) = &pending.machine else {
            panic!("the machine is still frozen: {:?}", pending.machine);
        };
        assert_eq!(
            machine.registers,
            vec![FrozenValue::Entity(target_back)],
            "the frozen register points at the restored target"
        );
    }

    /// The scene the editor saves by default is the one the runtime loads when
    /// `runtime.json` names none. The SDK's test pins its fallback against the
    /// same file.
    #[test]
    fn default_scene_path_is_the_one_the_runtime_falls_back_to() {
        let runtime_json: serde_json::Value =
            serde_json::from_str(include_str!("../../khora-sdk/tests/fixtures/runtime.json"))
                .expect("the fixture is valid JSON");
        assert_eq!(
            Some(DEFAULT_SCENE_REL),
            runtime_json["default_scene"].as_str()
        );
    }
}
