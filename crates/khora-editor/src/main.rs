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

mod chrome;
mod cmd_palette;
mod fonts;
mod mod_agents;
mod mod_gizmo;
mod ops;
mod panels;
mod scene_io;
mod theme;
mod util;
mod widgets;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::*;
use khora_sdk::WgpuRenderSystem;
use khora_sdk::RenderSystem;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{AgentProvider, EngineApp, GameWorld, InputEvent, ServiceRegistry};
use khora_sdk::{CommandHistory, DccService, PlayMode};
use khora_sdk::{EditorState, EditorCamera, EditorLogCapture, LogEntry, EditorShell, GizmoMode, PanelLocation};
use khora_sdk::editor_ui::viewport_texture::ViewportTextureHandle;
use khora_sdk::khora_core::ui::{EditorOverlay, OverlayScreenDescriptor};
use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::platform::KhoraWindow;
use khora_sdk::winit;

use chrome::{SpinePanel, StatusBarPanel, TitleBarPanel};
use cmd_palette::CommandPalettePanel;
use panels::{
    AssetBrowserPanel, ConsolePanel, ControlPlanePanel, PropertiesPanel, SceneTreePanel,
    ViewportPanel,
};

/// CLI project path passed via --project <path>.
static PROJECT_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

struct EditorApp {
    camera: Arc<Mutex<EditorCamera>>,
    editor_state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
    log_handle: Arc<Mutex<Vec<LogEntry>>>,
    shell: Option<Arc<Mutex<Box<dyn EditorShell>>>>,
    overlay: Option<Arc<Mutex<Box<dyn EditorOverlay>>>>,
    /// Cached `Arc<winit::window::Window>` retrieved from services in
    /// `setup()`. Needed because the overlay's `begin_frame` and
    /// `handle_window_event` both expect a winit window reference.
    raw_window: Option<Arc<winit::window::Window>>,
    /// Live monitor handle exposed by the engine (Phase 2.1). Cloned in
    /// `setup` from the ServiceRegistry; queried each frame to push CPU/GPU
    /// load, VRAM, draw calls and triangles into `EditorState::status`.
    monitors: Option<khora_sdk::MonitorRegistry>,
    /// Live agent registry (Phase 2.3). Used by the Control Plane workspace
    /// to enumerate agents instead of the previous mock list.
    agent_registry: Option<Arc<Mutex<khora_sdk::AgentRegistry>>>,
    /// DCC context handle (Phase 2.2). Locked each frame for the Control
    /// Plane summary bar — exposes mode, budget multiplier, hardware load.
    dcc_context: Option<Arc<std::sync::RwLock<khora_sdk::DccContext>>>,
    /// View info computed in `before_agents` and re-used by `after_agents`
    /// for gizmo rendering.
    last_view_info: Option<ViewInfo>,
    middle_down: bool,
    right_down: bool,
    shift_held: bool,
    ctrl_held: bool,
    prev_cursor: Option<(f32, f32)>,
    /// Last known cursor position in physical screen pixels — kept in sync
    /// with every `WindowEvent::CursorMoved` egui-winit reports. Used by
    /// `intercept_window_event` to test whether a `MouseInput` event (which
    /// has no position of its own) lands inside the 3D viewport rect.
    last_cursor_pos: Option<(f32, f32)>,
    last_frame_time: Instant,
    /// Editor viewport override — read by `RenderFlow` as fallback when no
    /// active scene `Camera` exists (i.e. Editing mode). Updated each frame
    /// in `before_agents` from the editor's free camera.
    viewport_override: khora_sdk::khora_data::render::EditorViewportOverride,
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

                "spawn_empty" => {
                    if let Ok(mut state) = self.editor_state.lock() {
                        state.pending_spawn = Some("Empty".to_owned());
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

// ─────────────────────────────────────────────────────────────────────
// EngineApp implementation
// ─────────────────────────────────────────────────────────────────────

impl EngineApp for EditorApp {
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
            overlay: None,
            raw_window: None,
            monitors: None,
            agent_registry: None,
            dcc_context: None,
            last_view_info: None,
            middle_down: false,
            right_down: false,
            shift_held: false,
            ctrl_held: false,
            prev_cursor: None,
            last_cursor_pos: None,
            last_frame_time: Instant::now(),
            viewport_override: khora_sdk::khora_data::render::EditorViewportOverride::new(),
        }
    }

    fn setup(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
        // Cache services.
        if let Some(camera) = services.get::<std::sync::Arc<std::sync::Mutex<EditorCamera>>>().cloned() {
            self.camera = camera;
        }

        // Cache the editor overlay (created by the bootstrap closure).
        self.overlay = services
            .get::<Arc<Mutex<Box<dyn EditorOverlay>>>>()
            .cloned();

        // Cache live engine handles (Phase 2 wiring) — both are cheap clones
        // of internal Arc-shared structures.
        self.monitors = services.get::<khora_sdk::MonitorRegistry>().cloned();
        self.agent_registry = services
            .get::<Arc<Mutex<khora_sdk::AgentRegistry>>>()
            .cloned();
        self.dcc_context = services
            .get::<Arc<std::sync::RwLock<khora_sdk::DccContext>>>()
            .cloned();

        // Cache the raw winit window so overlay calls can hand it back to egui.
        self.raw_window = services
            .get::<Arc<winit::window::Window>>()
            .cloned();

        let viewport_handle = services
            .get::<ViewportTextureHandle>()
            .copied()
            .unwrap_or(khora_sdk::PRIMARY_VIEWPORT);

        if let Some(shell_ref) = services
            .get::<std::sync::Arc<std::sync::Mutex<Box<dyn EditorShell>>>>()
            .cloned()
        {
            if let Ok(mut shell) = shell_ref.lock() {
                shell.set_editor_state(self.editor_state.clone());

                // ── Brand identity (theme + typefaces) ─────────
                let brand_theme = theme::khora_dark();
                shell.set_theme(brand_theme.clone());
                shell.set_fonts(fonts::load_pack());

                // ── Chrome (top, spine, status bar) ────────────
                shell.register_panel(
                    PanelLocation::TopBar,
                    Box::new(TitleBarPanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Spine,
                    Box::new(SpinePanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::StatusBar,
                    Box::new(StatusBarPanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );

                // ── Functional panels (dock body) ──────────────
                shell.register_panel(
                    PanelLocation::Left,
                    Box::new(SceneTreePanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Right,
                    Box::new(PropertiesPanel::new(
                        self.editor_state.clone(),
                        self.command_history.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Bottom,
                    Box::new(AssetBrowserPanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Bottom,
                    Box::new(ConsolePanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Center,
                    Box::new(ViewportPanel::new(
                        viewport_handle,
                        self.editor_state.clone(),
                        self.camera.clone(),
                        brand_theme.clone(),
                    )),
                );
                shell.register_panel(
                    PanelLocation::Center,
                    Box::new(ControlPlanePanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
                        self.agent_registry.clone(),
                        self.dcc_context.clone(),
                    )),
                );

                // ── Floating overlays ──────────────────────────
                shell.register_panel(
                    PanelLocation::Floating(100),
                    Box::new(CommandPalettePanel::new(
                        self.editor_state.clone(),
                        brand_theme.clone(),
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

                let project_json: Option<serde_json::Value> =
                    std::fs::read_to_string(path.join("project.json"))
                        .ok()
                        .and_then(|json| serde_json::from_str(&json).ok());
                let project_name = project_json
                    .as_ref()
                    .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from));
                let project_engine_version = project_json
                    .as_ref()
                    .and_then(|v| {
                        v.get("engine_version")
                            .and_then(|n| n.as_str())
                            .map(String::from)
                    });

                let git_branch = util::read_git_branch(&path);

                if let Ok(mut state) = self.editor_state.lock() {
                    state.project_folder = Some(path.to_string_lossy().to_string());
                    state.project_name = project_name.clone();
                    state.project_engine_version = project_engine_version.clone();
                    state.asset_entries = entries;
                    state.current_git_branch = git_branch.clone();
                    log::info!(
                        "Opened project '{}' from CLI: '{}' ({} assets, git: {}, engine: {})",
                        project_name.as_deref().unwrap_or("<unknown>"),
                        path.display(),
                        state.asset_entries.len(),
                        git_branch.as_deref().unwrap_or("none"),
                        project_engine_version.as_deref().unwrap_or("<unknown>")
                    );
                }
            } else {
                log::warn!("--project path does not exist: {}", project_path);
            }
        }

        // Auto-load the default scene, or create one if it doesn't exist.
        if let Some(project_path) = PROJECT_PATH.get().and_then(|p| p.as_ref()) {
            let project_root = std::path::Path::new(project_path);
            if project_root.exists() {
                scene_io::auto_load_or_create_default_scene(world, project_root);
            }
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        // Consume pending scene load (from asset browser double-click).
        let pending_load = self
            .editor_state
            .lock()
            .ok()
            .and_then(|mut s| s.pending_scene_load.take());
        if let Some(path) = pending_load {
            scene_io::load_scene_from(world, &path);
            if let Ok(mut state) = self.editor_state.lock() {
                state.current_scene_path = Some(path);
            }
        }

        // Read the live viewport rect once per frame; navigation tests
        // cursor positions against it instead of relying on the
        // `viewport_hovered` flag (which can be a frame stale and was
        // unreliable when switching modes).
        let (viewport_rect, play_mode) = self
            .editor_state
            .lock()
            .ok()
            .map(|s| (s.viewport_screen_rect, s.play_mode))
            .unwrap_or((None, PlayMode::Editing));
        let cursor_in_viewport = |x: f32, y: f32| {
            viewport_rect
                .map(|[rx, ry, rw, rh]| x >= rx && x < rx + rw && y >= ry && y < ry + rh)
                .unwrap_or(false)
        };
        // The editor camera is only navigable in Editing mode. In Play /
        // Paused, mouse motion over the viewport must NOT move the editor
        // camera — otherwise users see no visible difference between the
        // two modes (and the active scene camera is the one that should
        // render).
        let editor_cam_navigable = play_mode == PlayMode::Editing;

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

                    if key_code == "KeyK" && self.ctrl_held {
                        if let Ok(mut state) = self.editor_state.lock() {
                            state.command_palette_open = !state.command_palette_open;
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
                    if editor_cam_navigable && cursor_in_viewport(*x, *y) {
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
                    let in_view = self
                        .last_cursor_pos
                        .map(|(x, y)| cursor_in_viewport(x, y))
                        .unwrap_or(false);
                    if editor_cam_navigable && in_view {
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
            ops::process_reparents(world, &mut state);

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

            if let Some((entity, type_name)) = state.pending_add_component.take() {
                ops::add_component_to_entity(world, entity, &type_name);
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

            // Phase 2.1 — pull live telemetry into status. Only the fields
            // actually reported are updated; missing reports leave the
            // previous value untouched (status fields default to 0).
            if let Some(ref monitors) = self.monitors {
                use khora_sdk::MonitoredResourceType;
                for monitor in monitors.get_all_monitors() {
                    match monitor.resource_type() {
                        MonitoredResourceType::Vram => {
                            let r = monitor.get_usage_report();
                            state.status.vram_mb = r.current_bytes as f32 / (1024.0 * 1024.0);
                        }
                        MonitoredResourceType::Gpu => {
                            if let Some(g) = monitor.get_gpu_report() {
                                state.status.draw_calls = g.draw_calls;
                                state.status.triangles = g.triangles_rendered as u64;
                            }
                            if let Some(hw) = monitor.get_hardware_report() {
                                state.status.gpu_load = hw.gpu_load.unwrap_or(0.0);
                            }
                        }
                        MonitoredResourceType::Hardware => {
                            if let Some(hw) = monitor.get_hardware_report() {
                                state.status.cpu_load = hw.cpu_load;
                                if let Some(g) = hw.gpu_load {
                                    state.status.gpu_load = g;
                                }
                            }
                        }
                        MonitoredResourceType::SystemRam => {
                            let r = monitor.get_usage_report();
                            state.status.memory_used_mb =
                                r.current_bytes as f32 / (1024.0 * 1024.0);
                        }
                    }
                }
            }

            let status_copy = state.status.clone();
            drop(state);

            if let Some(ref shell) = self.shell {
                if let Ok(mut shell) = shell.lock() {
                    shell.set_status(status_copy);
                }
            }
        }
    }

    fn on_shutdown(&mut self) {
        log::info!("EditorApp: Shutting down");
    }

    fn intercept_window_event(
        &mut self,
        event: &dyn std::any::Any,
        _window: &dyn KhoraWindow,
    ) -> bool {
        let Some(overlay_arc) = self.overlay.as_ref() else {
            return false;
        };
        let Some(raw_window) = self.raw_window.as_ref() else {
            return false;
        };
        let Ok(mut overlay) = overlay_arc.lock() else {
            return false;
        };
        // egui *always* sees the event so it can keep its hover state in
        // sync; the override below only changes the "consumed" verdict so
        // the engine can ALSO process pointer events that land inside the
        // viewport rect (without the override, egui's CentralPanel swallows
        // every press/drag via `wants_pointer_input()`).
        let consumed_by_egui = overlay
            .handle_window_event((&**raw_window) as &dyn std::any::Any, event);

        let we = event.downcast_ref::<winit::event::WindowEvent>();
        let Some(we) = we else { return consumed_by_egui };
        use winit::event::WindowEvent;

        // Track the cursor position so MouseInput events (which carry no
        // position of their own) can be tested against the viewport rect.
        if let WindowEvent::CursorMoved { position, .. } = we {
            self.last_cursor_pos = Some((position.x as f32, position.y as f32));
        }

        let is_pointer_event = matches!(
            we,
            WindowEvent::MouseInput { .. }
                | WindowEvent::CursorMoved { .. }
                | WindowEvent::MouseWheel { .. }
        );
        if !is_pointer_event {
            return consumed_by_egui;
        }

        // Pull the current viewport rect (set every frame by viewport.rs::ui)
        // and the cursor position the event refers to.
        let viewport_rect = self
            .editor_state
            .lock()
            .ok()
            .and_then(|s| s.viewport_screen_rect);
        let pos = match we {
            WindowEvent::CursorMoved { position, .. } => {
                Some((position.x as f32, position.y as f32))
            }
            _ => self.last_cursor_pos,
        };

        if let (Some([rx, ry, rw, rh]), Some((cx, cy))) = (viewport_rect, pos) {
            let in_viewport = cx >= rx && cx < rx + rw && cy >= ry && cy < ry + rh;
            if in_viewport {
                return false;
            }
        }
        consumed_by_egui
    }

    fn before_frame(
        &mut self,
        _world: &mut GameWorld,
        services: &ServiceRegistry,
        window: &dyn KhoraWindow,
    ) {
        // Switch the renderer to offscreen-viewport mode BEFORE
        // `begin_render_frame` runs, so the per-frame `FrameContext` receives
        // the viewport color/depth targets (instead of the swapchain) and
        // agents paint into the texture displayed by the egui viewport panel.
        if let Some(rs_arc) = services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>().cloned() {
            if let Ok(mut rs) = rs_arc.lock() {
                rs.set_render_to_viewport(true);
            }
        }

        let Some(overlay_arc) = self.overlay.as_ref() else { return };
        let Some(raw_window) = self.raw_window.as_ref() else { return };

        let (w_px, h_px) = window.inner_size();
        let screen = OverlayScreenDescriptor {
            width_px: w_px,
            height_px: h_px,
            scale_factor: window.scale_factor() as f32,
        };

        if let Ok(mut overlay) = overlay_arc.lock() {
            overlay.begin_frame((&**raw_window) as &dyn std::any::Any, screen);
        }

        if let Some(shell) = self.shell.as_ref() {
            if let Ok(mut shell) = shell.lock() {
                shell.show_frame();
            }
        }
    }

    fn before_agents(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
        // Compute the active `ViewInfo` and clear the offscreen viewport
        // (background + infinite grid). Cached for `after_agents` so gizmos
        // use the same projection.
        //
        // Camera selection:
        //  - Editing mode → editor camera (free orbit/pan/zoom).
        //  - Play / Paused → first active scene Camera (so the viewport
        //    actually shows the game's view, not the editor's).
        self.last_view_info = None;

        let Some(rs_arc) = services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>().cloned() else {
            return;
        };
        let Ok(mut rs) = rs_arc.lock() else { return };
        let Some(wgpu_rs) = rs.as_any_mut().downcast_mut::<WgpuRenderSystem>() else {
            return;
        };

        let (vw, vh) = wgpu_rs.viewport_size();
        let play_mode = self
            .editor_state
            .lock()
            .ok()
            .map(|s| s.play_mode)
            .unwrap_or(PlayMode::Editing);

        let view_info = match play_mode {
            PlayMode::Editing => match self.camera.lock() {
                Ok(cam) => cam.view_info(vw as f32, vh as f32),
                Err(_) => ViewInfo::default(),
            },
            PlayMode::Playing | PlayMode::Paused => {
                use khora_sdk::khora_data::render::extract_active_camera_view;
                extract_active_camera_view(world.inner_world())
                    .or_else(|| {
                        // No active scene camera (shouldn't happen because
                        // sync_scene_cameras_for_mode promotes one) — fall
                        // back to the editor cam so the user still sees
                        // something instead of a blank screen.
                        self.camera
                            .lock()
                            .ok()
                            .map(|cam| cam.view_info(vw as f32, vh as f32))
                    })
                    .unwrap_or_default()
            }
        };

        // ── Donate the active view to RenderWorld.views ────────
        // The render lanes (SimpleUnlit / LitForward / ForwardPlus) all
        // bail to a clear-only pass when `RenderWorld.views.first()` is
        // None — and `extract_views` (run once per frame in
        // Publish the active view into the EditorViewportOverride so
        // RenderFlow can fall back to it when no scene Camera is active
        // (Editing mode forces every scene camera inactive). RenderFlow
        // appends the override to RenderWorld.views during its `project`
        // step on the next scheduler tick.
        self.viewport_override.set(Some(
            khora_sdk::khora_data::render::ExtractedView {
                view_proj: view_info.view_projection_matrix(),
                position: view_info.camera_position,
            },
        ));

        let clear = khora_sdk::prelude::math::LinearRgba::new(0.15, 0.15, 0.18, 1.0);
        if let Err(e) = wgpu_rs.render_viewport(clear, &view_info) {
            log::error!("editor: render_viewport failed: {e:?}");
        }
        wgpu_rs.prepare_frame(&view_info);
        self.last_view_info = Some(view_info);
    }

    fn after_agents(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
        let Some(rs_arc) = services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>().cloned() else {
            return;
        };

        // Render gizmos for the current selection on top of the 3D scene.
        if let Some(view_info) = self.last_view_info.as_ref() {
            let gizmo_lines = if let Ok(state) = self.editor_state.lock() {
                if state.selection.is_empty() {
                    Vec::new()
                } else {
                    crate::mod_gizmo::collect_gizmo_lines(world, &state, view_info)
                }
            } else {
                Vec::new()
            };

            if !gizmo_lines.is_empty() {
                if let Ok(mut rs) = rs_arc.lock() {
                    if let Some(wgpu_rs) = rs.as_any_mut().downcast_mut::<WgpuRenderSystem>() {
                        if let Err(e) = wgpu_rs.render_gizmos(view_info, &gizmo_lines) {
                            log::warn!("editor: render_gizmos failed: {e:?}");
                        }
                    }
                }
            }
        }

        // Present the egui overlay last so the dock + panels paint over the
        // 3D scene encoded by the agents.
        let Some(overlay_arc) = self.overlay.as_ref() else { return };
        let Some(raw_window) = self.raw_window.as_ref() else { return };

        let inner = (**raw_window).inner_size();
        let screen = OverlayScreenDescriptor {
            width_px: inner.width,
            height_px: inner.height,
            scale_factor: (**raw_window).scale_factor() as f32,
        };

        {
            let mut rs = match rs_arc.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            // Switch back to swapchain so the egui overlay (dock + panels +
            // viewport panel that displays the offscreen texture) paints onto
            // the presented surface.
            rs.set_render_to_viewport(false);
            let mut overlay = match overlay_arc.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let overlay_ref: &mut dyn EditorOverlay = &mut **overlay;
            if let Err(e) = rs.render_overlay(overlay_ref, screen) {
                log::error!("editor: render_overlay failed: {e:?}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// AgentProvider implementation
// ─────────────────────────────────────────────────────────────────────

impl AgentProvider for EditorApp {
    fn register_agents(&self, dcc: &DccService, services: &mut ServiceRegistry) {
        // Insert EditorState and EditorCamera into services so agents can access them.
        services.insert(self.editor_state.clone());
        services.insert(self.camera.clone());

        // Editor viewport override — RenderFlow reads it as fallback when no
        // scene Camera is active (Editing mode).
        services.insert(self.viewport_override.clone());

        // Register editor-specific agents with mode filtering
        mod_agents::register_editor_agents(dcc);
    }
}

impl khora_sdk::PhaseProvider for EditorApp {
    fn custom_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
    }

    fn removed_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
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

    run_winit::<WinitWindowProvider, EditorApp>(|window, services, event_loop_any| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        services.insert(rs.graphics_device());

        // Build the editor overlay (egui) + shell (dock + panels) so the
        // editor UI renders on top of the 3D scene each frame.
        let event_loop = event_loop_any
            .downcast_ref::<winit::event_loop::ActiveEventLoop>()
            .expect("editor: bootstrap expects a winit ActiveEventLoop");
        let theme = khora_sdk::khora_core::ui::editor::EditorTheme::default();
        match rs.create_editor_overlay_and_shell(
            event_loop,
            khora_sdk::khora_lanes::render_lane::shaders::EGUI_WGSL,
            khora_sdk::khora_lanes::render_lane::shaders::GRID_WGSL,
            theme,
            khora_sdk::PRIMARY_VIEWPORT,
        ) {
            Ok((overlay, shell)) => {
                let overlay: Box<dyn EditorOverlay> = Box::new(overlay);
                let shell: Box<dyn EditorShell> = Box::new(shell);
                services.insert(Arc::new(Mutex::new(overlay)));
                services.insert(Arc::new(Mutex::new(shell)));
                log::info!("editor: overlay + shell created");
            }
            Err(e) => {
                log::error!("editor: failed to create overlay+shell: {e:?}");
            }
        }

        let rs: Box<dyn RenderSystem> = Box::new(rs);
        services.insert(Arc::new(Mutex::new(rs)));
    })?;
    Ok(())
}
