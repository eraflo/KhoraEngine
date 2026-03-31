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

//! Khora Engine Editor application entry point.

mod ops;
mod panels;
mod scene_io;
mod util;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_core::ui::editor::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::prelude::*;
use khora_sdk::{AppContext, Application, Engine, GameWorld, InputEvent, PRIMARY_VIEWPORT};

use panels::{AssetBrowserPanel, ConsolePanel, PropertiesPanel, SceneTreePanel, ViewportPanel};

/// CLI project path passed via --project <path>.
static PROJECT_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

struct EditorApp {
    camera: Arc<Mutex<EditorCamera>>,
    editor_state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
    log_handle: Arc<Mutex<Vec<LogEntry>>>,
    shell: Option<Arc<Mutex<Box<dyn EditorShell>>>>,
    middle_down: bool,
    right_down: bool,
    shift_held: bool,
    ctrl_held: bool,
    prev_cursor: Option<(f32, f32)>,
    last_frame_time: Instant,
}

impl EditorApp {
    fn process_menu_actions(&mut self, world: &mut GameWorld) {
        let wants_browse = self
            .editor_state
            .lock()
            .ok()
            .map(|mut s| std::mem::replace(&mut s.pending_browse_project_folder, false))
            .unwrap_or(false);

        if wants_browse {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let entries = scene_io::scan_project_folder(&path);
                if let Ok(mut state) = self.editor_state.lock() {
                    state.project_folder = Some(path.to_string_lossy().to_string());
                    state.asset_entries = entries;
                    log::info!(
                        "Asset browser: scanned '{}' - {} assets found",
                        path.display(),
                        state.asset_entries.len()
                    );
                }
            }
        }

        let action = self
            .editor_state
            .lock()
            .ok()
            .and_then(|mut s| s.pending_menu_action.take());

        if let Some(action) = action {
            match action.as_str() {
                "new_scene" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        let all: Vec<EntityId> = world.iter_entities().collect();
                        for entity in &all {
                            world.despawn(*entity);
                        }
                        state.clear_selection();
                        state.inspected = None;
                        state.scene_roots.clear();
                        state.entity_count = 0;
                        state.play_mode = PlayMode::Editing;
                        state.scene_snapshot = None;
                        log::info!("New scene created (cleared {} entities)", all.len());
                    }
                }
                "undo" => {
                    if let Ok(mut history) = self.command_history.lock() {
                        if let Some(edit) = history.undo() {
                            if let Ok(mut state) = self.editor_state.lock() {
                                state.push_edit(edit);
                            }
                        }
                    }
                }
                "redo" => {
                    if let Ok(mut history) = self.command_history.lock() {
                        if let Some(edit) = history.redo() {
                            if let Ok(mut state) = self.editor_state.lock() {
                                state.push_edit(edit);
                            }
                        }
                    }
                }
                "delete" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        ops::delete_selection(world, &mut state);
                    }
                }
                "quit" => {
                    log::info!("Quit requested from menu");
                    std::process::exit(0);
                }

                "play" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        match state.play_mode {
                            PlayMode::Editing => {
                                state.scene_snapshot = Some(scene_io::snapshot_scene(world));
                                state.play_mode = PlayMode::Playing;
                                log::info!("Play mode: started");
                            }
                            PlayMode::Paused => {
                                state.play_mode = PlayMode::Playing;
                                log::info!("Play mode: resumed");
                            }
                            _ => {}
                        }
                    }
                }
                "pause" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        if state.play_mode == PlayMode::Playing {
                            state.play_mode = PlayMode::Paused;
                            log::info!("Play mode: paused");
                        }
                    }
                }
                "stop" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        if state.play_mode == PlayMode::Playing
                            || state.play_mode == PlayMode::Paused
                        {
                            if let Some(snapshot) = state.scene_snapshot.take() {
                                drop(state);
                                scene_io::restore_scene(world, &snapshot);
                                if let Ok(mut state) = self.editor_state.lock() {
                                    state.play_mode = PlayMode::Editing;
                                }
                            } else {
                                state.play_mode = PlayMode::Editing;
                            }
                            log::info!("Play mode: stopped - scene restored");
                        }
                    }
                }

                "save" => {
                    let path = self
                        .editor_state
                        .lock()
                        .ok()
                        .and_then(|s| s.current_scene_path.clone());
                    if let Some(path) = path {
                        scene_io::save_scene_to(world, &path);
                    } else if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Khora Scene", &["kscene"])
                        .save_file()
                    {
                        let path = path.to_string_lossy().to_string();
                        scene_io::save_scene_to(world, &path);
                        if let Ok(mut state) = self.editor_state.lock() {
                            state.current_scene_path = Some(path);
                        }
                    }
                }
                "save_as" => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Khora Scene", &["kscene"])
                        .save_file()
                    {
                        let path = path.to_string_lossy().to_string();
                        scene_io::save_scene_to(world, &path);
                        if let Ok(mut state) = self.editor_state.lock() {
                            state.current_scene_path = Some(path);
                        }
                    }
                }
                "open" => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Khora Scene", &["kscene"])
                        .pick_file()
                    {
                        let path = path.to_string_lossy().to_string();
                        scene_io::load_scene_from(world, &path);
                        if let Ok(mut state) = self.editor_state.lock() {
                            state.current_scene_path = Some(path);
                            state.clear_selection();
                            state.inspected = None;
                        }
                    }
                }

                "documentation" => {
                    let _ = open::that("https://github.com/eraflo/KhoraEngine");
                    log::info!("Opening documentation in browser");
                }
                "about" => {
                    log::info!("Khora Engine v0.1.0-dev - experimental game engine");
                }

                "preferences" | "reset_layout" => {
                    log::info!("Menu action '{}' (not yet implemented)", action);
                }

                other => {
                    log::info!("Unhandled menu action: {}", other);
                }
            }
        }
    }
}

impl Application for EditorApp {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "Khora Engine Editor".to_owned(),
            icon: Some(load_logo_icon()),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        let editor_state = Arc::new(Mutex::new(EditorState::default()));
        let command_history = Arc::new(Mutex::new(CommandHistory::default()));

        let (capture, log_handle) = EditorLogCapture::new();
        let _ = log::set_boxed_logger(Box::new(capture));
        log::set_max_level(log::LevelFilter::Debug);

        Self {
            camera: Arc::new(Mutex::new(EditorCamera::default())),
            editor_state,
            command_history,
            log_handle,
            shell: None,
            middle_down: false,
            right_down: false,
            shift_held: false,
            ctrl_held: false,
            prev_cursor: None,
            last_frame_time: Instant::now(),
        }
    }

    fn setup(&mut self, world: &mut GameWorld, ctx: &mut AppContext) {
        // Cache services from the AppContext.
        if let Some(camera) = ctx.services.get::<Arc<Mutex<EditorCamera>>>().cloned() {
            self.camera = camera;
        }

        let viewport_handle = ctx
            .services
            .get::<ViewportTextureHandle>()
            .copied()
            .unwrap_or(PRIMARY_VIEWPORT);

        if let Some(shell_ref) = ctx
            .services
            .get::<Arc<Mutex<Box<dyn EditorShell>>>>()
            .cloned()
        {
            if let Ok(mut shell) = shell_ref.lock() {
                shell.set_editor_state(self.editor_state.clone());

                shell.register_panel(
                    PanelLocation::Left,
                    Box::new(SceneTreePanel::new(self.editor_state.clone())),
                );
                shell.register_panel(
                    PanelLocation::Right,
                    Box::new(PropertiesPanel::new(
                        self.editor_state.clone(),
                        self.command_history.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Bottom,
                    Box::new(AssetBrowserPanel::new(self.editor_state.clone())),
                );
                shell.register_panel(
                    PanelLocation::Bottom,
                    Box::new(ConsolePanel::new(self.editor_state.clone())),
                );
                shell.register_panel(
                    PanelLocation::Center,
                    Box::new(ViewportPanel::new(
                        viewport_handle,
                        self.editor_state.clone(),
                        self.camera.clone(),
                    )),
                );
                log::info!("EditorApp: panels registered with shell.");
            }
            self.shell = Some(shell_ref);
        } else {
            log::warn!("EditorApp: no EditorShell found in ServiceRegistry.");
        }
        if let Some(Some(project_path)) = PROJECT_PATH.get() {
            let path = std::path::PathBuf::from(project_path);
            if path.exists() {
                let entries = scene_io::scan_project_folder(&path);

                let project_name = std::fs::read_to_string(path.join("project.json"))
                    .ok()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                    .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from));

                if let Ok(mut state) = self.editor_state.lock() {
                    state.project_folder = Some(path.to_string_lossy().to_string());
                    state.project_name = project_name.clone();
                    state.asset_entries = entries;
                    log::info!(
                        "Opened project '{}' from CLI: '{}' ({} assets)",
                        project_name.as_deref().unwrap_or("<unknown>"),
                        path.display(),
                        state.asset_entries.len()
                    );
                }
            } else {
                log::warn!("--project path does not exist: {}", project_path);
            }
        }

        let has_entities = world.iter_entities().next().is_some();
        if !has_entities {
            let cam = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0);
            world.spawn((
                Transform::new(Vec3::new(0.0, 5.0, 10.0), Quaternion::IDENTITY, Vec3::ONE),
                GlobalTransform::identity(),
                Name::new("Main Camera"),
                cam,
            ));

            world.spawn((
                Transform::new(Vec3::new(0.0, 10.0, 0.0), Quaternion::IDENTITY, Vec3::ONE),
                GlobalTransform::identity(),
                Name::new("Directional Light"),
                Light::directional(),
            ));

            log::info!("Default scene created: Camera + Directional Light");
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        let viewport_hovered = self
            .editor_state
            .lock()
            .map(|s| s.viewport_hovered)
            .unwrap_or(false);

        for input in inputs {
            match input {
                InputEvent::MouseButtonPressed { button } => match button {
                    MouseButton::Middle => self.middle_down = true,
                    MouseButton::Right => self.right_down = true,
                    _ => {}
                },
                InputEvent::MouseButtonReleased { button } => match button {
                    MouseButton::Middle => {
                        self.middle_down = false;
                        self.prev_cursor = None;
                    }
                    MouseButton::Right => {
                        self.right_down = false;
                        self.prev_cursor = None;
                    }
                    _ => {}
                },
                InputEvent::KeyPressed { key_code } => {
                    if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                        self.shift_held = true;
                    }
                    if key_code == "ControlLeft" || key_code == "ControlRight" {
                        self.ctrl_held = true;
                    }

                    if !self.ctrl_held {
                        if let Ok(mut state) = self.editor_state.lock() {
                            match key_code.as_str() {
                                "KeyQ" => state.gizmo_mode = GizmoMode::Select,
                                "KeyW" => state.gizmo_mode = GizmoMode::Move,
                                "KeyE" => state.gizmo_mode = GizmoMode::Rotate,
                                "KeyR" => state.gizmo_mode = GizmoMode::Scale,
                                _ => {}
                            }
                        }
                    }

                    if key_code == "Delete" {
                        if let Ok(mut state) = self.editor_state.lock() {
                            ops::delete_selection(world, &mut state);
                        }
                    }

                    if key_code == "KeyZ" && self.ctrl_held {
                        if let Ok(mut history) = self.command_history.lock() {
                            if let Some(edit) = history.undo() {
                                if let Ok(mut state) = self.editor_state.lock() {
                                    state.push_edit(edit);
                                }
                            }
                        }
                    }

                    if key_code == "KeyY" && self.ctrl_held {
                        if let Ok(mut history) = self.command_history.lock() {
                            if let Some(edit) = history.redo() {
                                if let Ok(mut state) = self.editor_state.lock() {
                                    state.push_edit(edit);
                                }
                            }
                        }
                    }
                }
                InputEvent::KeyReleased { key_code } => {
                    if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                        self.shift_held = false;
                    }
                    if key_code == "ControlLeft" || key_code == "ControlRight" {
                        self.ctrl_held = false;
                    }
                }
                InputEvent::MouseMoved { x, y } => {
                    if viewport_hovered {
                        if let Some((px, py)) = self.prev_cursor {
                            let dx = x - px;
                            let dy = y - py;

                            if let Ok(mut cam) = self.camera.lock() {
                                if self.right_down || (self.middle_down && self.shift_held) {
                                    cam.pan(dx, dy);
                                } else if self.middle_down {
                                    cam.orbit(dx, dy);
                                }
                            }
                        }
                    }
                    self.prev_cursor = Some((*x, *y));
                }
                InputEvent::MouseWheelScrolled { delta_y, .. } => {
                    if viewport_hovered {
                        if let Ok(mut cam) = self.camera.lock() {
                            cam.zoom(*delta_y);
                        }
                    }
                }
            }
        }

        self.process_menu_actions(world);

        if let Ok(mut state) = self.editor_state.lock() {
            ops::apply_edits(world, &mut state);
        }

        let play_mode = self
            .editor_state
            .lock()
            .map(|s| s.play_mode)
            .unwrap_or(PlayMode::Editing);
        ops::sync_scene_cameras_for_mode(world, play_mode);

        if let Ok(mut state) = self.editor_state.lock() {
            state.ctrl_held = self.ctrl_held;

            ops::process_spawns(world, &mut state);

            if let Some((entity, new_name)) = state.pending_rename.take() {
                if let Some(name) = world.get_component_mut::<Name>(entity) {
                    *name = Name::new(&new_name);
                    log::info!("Renamed entity {:?} to '{}'", entity, new_name);
                }
            }

            if let Some(entity) = state.pending_delete.take() {
                world.despawn(entity);
                state.selection.remove(&entity);
                if state.inspected.as_ref().is_some_and(|i| i.entity == entity) {
                    state.inspected = None;
                }
                log::info!("Deleted entity {:?}", entity);
            }

            if let Some(entity) = state.pending_duplicate.take() {
                ops::duplicate_entity(world, entity, &mut state);
            }

            ops::extract_scene_tree(world, &mut state);
            ops::extract_inspected(world, &mut state);

            if let Ok(log_entries) = self.log_handle.lock() {
                state.log_entries.clone_from(&log_entries);
            }

            let now = Instant::now();
            let dt = now.duration_since(self.last_frame_time).as_secs_f32();
            self.last_frame_time = now;
            state.status.frame_time_ms = dt * 1000.0;
            state.status.fps = if dt > 0.0 { 1.0 / dt } else { 0.0 };
            state.status.entity_count = state.entity_count;

            let status_copy = state.status.clone();
            drop(state);

            if let Some(ref shell) = self.shell {
                if let Ok(mut shell) = shell.lock() {
                    shell.set_status(status_copy);
                }
            }
        }
    }
}

fn load_logo_icon() -> WindowIcon {
    let png_bytes = include_bytes!("../assets/khora_small_logo.png");
    match image::load_from_memory(png_bytes) {
        Ok(img) => {
            let rgba_img = img.to_rgba8();
            let (w, h) = rgba_img.dimensions();
            WindowIcon {
                rgba: rgba_img.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(e) => {
            log::warn!("Failed to decode logo PNG: {}", e);
            WindowIcon {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let project = args
        .windows(2)
        .find(|w| w[0] == "--project")
        .map(|w| w[1].clone());
    let _ = PROJECT_PATH.set(project);

    Engine::run::<EditorApp>()?;
    Ok(())
}
