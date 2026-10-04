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

//! The editor's engine mode: it starts editing, which is not the game, and
//! each frame's mode follows the transport.

use khora_sdk::{EngineApp, EngineMode, PlayMode};

use super::{engine_mode_for, EditorApp};

fn editor_mode() -> EngineMode {
    EngineMode::Custom("editor".to_owned())
}

/// **The editor is not the game.** It starts in its own mode, so no script
/// runs before anyone presses Play.
#[test]
fn the_editor_starts_in_its_own_mode() {
    let app = <EditorApp as EngineApp>::new();

    assert_eq!(app.initial_mode(), editor_mode());
}

/// Editing is the editor's mode; Play is the game.
#[test]
fn editing_is_the_editors_mode_and_playing_is_the_game() {
    assert_eq!(engine_mode_for(PlayMode::Editing), editor_mode());
    assert_eq!(engine_mode_for(PlayMode::Playing), EngineMode::Playing);
}

/// **A paused game is still the game.** Its scripts run — at a delta of
/// zero — as a pause menu needs.
#[test]
fn a_paused_game_is_still_the_game() {
    assert_eq!(engine_mode_for(PlayMode::Paused), EngineMode::Playing);
}

/// The inspector's view of the inspected entity against its instance's
/// prefab, kept across frames: it follows what the world says the entity
/// belongs to.
mod prefab_view {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use khora_sdk::editor_ui::{EditorState, InspectedEntity};
    use khora_sdk::khora_data::ecs::{PrefabInstance, World};
    use khora_sdk::khora_data::scene::{capture_subtree, write_scene_file, CompactEncoding};
    use khora_sdk::prelude::ecs::*;
    use khora_sdk::prelude::math::Vec3;
    use khora_sdk::{GameWorld, MetricsRegistry};

    use super::super::{attach_prefab_view, PrefabView};
    use crate::commands::process_pending_prefab_spawn;
    use crate::project_vfs::ProjectVfs;

    /// A project on disk holding a turret prefab — a root and a barrel —
    /// removed when the test ends.
    struct Project {
        root: PathBuf,
        vfs: Arc<Mutex<ProjectVfs>>,
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn project(name: &str) -> Project {
        let mut world = World::new();
        let root = world.spawn((Transform::identity(), Name::new("Turret")));
        let barrel = world.spawn((
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            Name::new("Barrel"),
        ));
        world.set_parent(barrel, Some(root));
        world.mark_authored(root).expect("authored");
        world.mark_authored(barrel).expect("authored");
        let record = capture_subtree(&world, root).expect("the turret captures");

        let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("an old project is removed");
        }
        let prefabs = dir.join("assets").join("prefabs");
        std::fs::create_dir_all(&prefabs).expect("the prefab folder");
        let file = write_scene_file(&record, &CompactEncoding).expect("the prefab writes");
        std::fs::write(prefabs.join("turret.kprefab"), file.to_bytes())
            .expect("the prefab is written");
        let vfs = ProjectVfs::open(dir.clone(), Arc::new(MetricsRegistry::new()))
            .expect("the project opens");
        Project {
            root: dir,
            vfs: Arc::new(Mutex::new(vfs)),
        }
    }

    /// A frame's inspection of `entity`: the view attached to a fresh
    /// inspected entity, as `extract_inspected` leaves it each frame.
    fn inspect(
        view: &mut Option<PrefabView>,
        project: &Project,
        world: &GameWorld,
        entity: EntityId,
    ) -> bool {
        let mut state = EditorState {
            inspected: Some(InspectedEntity {
                entity,
                name: String::new(),
                components_json: Vec::new(),
                prefab: None,
            }),
            ..EditorState::default()
        };
        attach_prefab_view(view, Some(&project.vfs), world, &mut state);
        state
            .inspected
            .and_then(|inspected| inspected.prefab)
            .is_some()
    }

    /// The barrel the author moves out of its instance, under an entity of
    /// the scene, is no part of the instance any more — on the next frame,
    /// though neither the selection nor the project's files changed.
    #[test]
    fn a_member_moved_out_of_its_instance_loses_its_prefab_view() {
        let project = project("prefab-view-reparent");
        let mut world = GameWorld::new();
        let state = Arc::new(Mutex::new(EditorState::default()));
        state.lock().expect("the editor state").pending_prefab_spawn =
            Some(("prefabs/turret.kprefab".to_owned(), None));
        process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
        let root = world
            .iter_entities()
            .find(|e| world.get_component::<PrefabInstance>(*e).is_some())
            .expect("the turret spawns");
        let barrel = world
            .iter_entities()
            .find(|e| {
                world
                    .get_component::<Parent>(*e)
                    .is_some_and(|parent| parent.0 == root)
            })
            .expect("the barrel hangs under the root");
        let mut view = None;
        assert!(inspect(&mut view, &project, &world, barrel), "a member");

        let base = world.spawn((Transform::identity(), Name::new("Base")));
        world.set_parent(barrel, Some(base));

        assert!(
            !inspect(&mut None, &project, &world, barrel),
            "a view built afresh"
        );
        assert!(
            !inspect(&mut view, &project, &world, barrel),
            "moved out of the instance, the barrel is still seen as its member"
        );
    }

    /// An edit that leaves the hierarchy and the project's files as they
    /// were keeps the prefab's expansion: it is not read again. The prefab's
    /// file taken away behind the project's back shows it — the view stays.
    #[test]
    fn a_component_edit_keeps_the_prefab_view() {
        let project = project("prefab-view-kept");
        let mut world = GameWorld::new();
        let state = Arc::new(Mutex::new(EditorState::default()));
        state.lock().expect("the editor state").pending_prefab_spawn =
            Some(("prefabs/turret.kprefab".to_owned(), None));
        process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
        let root = world
            .iter_entities()
            .find(|e| world.get_component::<PrefabInstance>(*e).is_some())
            .expect("the turret spawns");
        let mut view = None;
        assert!(inspect(&mut view, &project, &world, root), "the root");

        world
            .get_component_mut::<Transform>(root)
            .expect("placed")
            .scale = Vec3::new(2.0, 2.0, 2.0);
        std::fs::remove_file(
            project
                .root
                .join("assets")
                .join("prefabs")
                .join("turret.kprefab"),
        )
        .expect("the prefab file is taken away");

        assert!(
            !inspect(&mut None, &project, &world, root),
            "a view built afresh cannot read the prefab"
        );
        assert!(
            inspect(&mut view, &project, &world, root),
            "the prefab was read again for an edit that changed nothing it depends on"
        );
    }
}
