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

mod mod_agents;
mod mod_gizmo;
mod mod_mode;
mod mod_telemetry;
mod ops;
mod panels;
mod scene_io;
mod util;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::{Quaternion, Vec3};
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

use panels::{AssetBrowserPanel, ConsolePanel, PropertiesPanel, SceneTreePanel, ViewportPanel};

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
    /// View info computed in `before_agents` and re-used by `after_agents`
    /// for gizmo rendering.
    last_view_info: Option<ViewInfo>,
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
            last_view_info: None,
            middle_down: false,
            right_down: false,
            shift_held: false,
            ctrl_held: false,
            prev_cursor: None,
            last_frame_time: Instant::now(),
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
        overlay.handle_window_event((&**raw_window) as &dyn std::any::Any, event)
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

    fn before_agents(&mut self, _world: &mut GameWorld, services: &ServiceRegistry) {
        // Compute the editor camera's `ViewInfo` and clear the offscreen
        // viewport (background + infinite grid). Cached for `after_agents`
        // so gizmos use the same projection.
        self.last_view_info = None;

        let Some(rs_arc) = services.get::<Arc<Mutex<Box<dyn RenderSystem>>>>().cloned() else {
            return;
        };
        let Ok(mut rs) = rs_arc.lock() else { return };
        let Some(wgpu_rs) = rs.as_any_mut().downcast_mut::<WgpuRenderSystem>() else {
            return;
        };

        let (vw, vh) = wgpu_rs.viewport_size();
        let view_info = match self.camera.lock() {
            Ok(cam) => cam.view_info(vw as f32, vh as f32),
            Err(_) => ViewInfo::default(),
        };

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
