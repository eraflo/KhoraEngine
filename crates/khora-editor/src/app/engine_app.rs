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

//! The editor as an `EngineApp`: its hooks into the engine's frame.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_sdk::khora_core::platform::KhoraWindow;
use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::ui::{EditorOverlay, OverlayScreenDescriptor};
use khora_sdk::prelude::*;
use khora_sdk::{winit, RenderSystem, WgpuRenderSystem};
use khora_sdk::{CommandHistory, PlayMode};
use khora_sdk::{EditorCamera, EditorLogCapture, EditorState};
use khora_sdk::{EngineApp, GameWorld, InputEvent, Runtime};

use super::EditorApp;
use crate::bootstrap::load_logo_icon;
use crate::input::InputState;
use crate::{commands, hot_reload, input, mod_gizmo, ops};

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
            input: InputState::default(),
            wireframe_enabled: false,
            last_frame_time: Instant::now(),
            viewport_override: khora_sdk::khora_data::render::EditorViewportOverride::new(),
            project_vfs: None,
        }
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        self.cache_services(runtime);
        self.register_panels(runtime);
        self.open_cli_project(world);
        // The editor ground grid (`GridConfig`) is toggled per-frame in
        // `before_agents` based on `PlayMode` — it is editor chrome and
        // must hide in Play / Paused, like the selection gizmos.
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        // Consume pending scene load (from asset browser double-click).
        let pending_load = self
            .editor_state
            .lock()
            .ok()
            .and_then(|mut s| s.pending_scene_load.take());
        if let Some(path) = pending_load {
            let abs = std::path::PathBuf::from(&path);
            commands::load_scene_dispatch(
                self.project_vfs.as_ref(),
                world,
                &self.editor_state,
                &abs,
            );
            if let Ok(mut state) = self.editor_state.lock() {
                state.current_scene_path = Some(path);
            }
        }

        input::process_events(
            &mut self.input,
            inputs,
            world,
            &self.editor_state,
            &self.camera,
        );

        // Wireframe toggle is editor-view state, not a scene command — handle
        // it here (flipping our own flag) before the scene-command dispatch so
        // it does not fall through to the "unhandled action" log. The flag is
        // pushed into the shared `WireframeConfig` in `before_agents`.
        if let Ok(mut s) = self.editor_state.lock() {
            if s.pending_menu_action.as_deref() == Some("toggle_wireframe") {
                s.pending_menu_action = None;
                self.wireframe_enabled = !self.wireframe_enabled;
            }
        }

        commands::process_menu_actions(
            &mut self.project_vfs,
            &self.editor_state,
            &self.command_history,
            world,
        );

        commands::process_pending_save_as_prefab(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );
        commands::process_pending_prefab_spawn(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );
        commands::process_pending_save_as_material(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );
        commands::process_pending_assign_material(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );
        commands::process_pending_asset_file_ops(self.project_vfs.as_ref(), &self.editor_state);
        commands::process_pending_spawn_mesh_asset(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );
        commands::process_pending_assign_texture(
            self.project_vfs.as_ref(),
            world,
            &self.editor_state,
        );

        if let Ok(mut state) = self.editor_state.lock() {
            ops::apply_edits(world, &mut state);
        }

        let play_mode = self
            .editor_state
            .lock()
            .map(|s| s.play_mode)
            .unwrap_or(PlayMode::Editing);
        ops::sync_scene_cameras_for_mode(world, play_mode);

        self.tick_state(world);
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
        // viewport rect (without the override, egui's CentralPanel
        // swallows every press/drag via `wants_pointer_input()`).
        let consumed_by_egui =
            overlay.handle_window_event((&**raw_window) as &dyn std::any::Any, event);

        let we = event.downcast_ref::<winit::event::WindowEvent>();
        let Some(we) = we else {
            return consumed_by_egui;
        };
        use winit::event::WindowEvent;

        // Track the cursor position so MouseInput events (which carry no
        // position of their own) can be tested against the viewport rect.
        if let WindowEvent::CursorMoved { position, .. } = we {
            self.input.last_cursor_pos = Some((position.x as f32, position.y as f32));
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

        let viewport_rect = self
            .editor_state
            .lock()
            .ok()
            .and_then(|s| s.viewport_screen_rect);
        let pos = match we {
            WindowEvent::CursorMoved { position, .. } => {
                Some((position.x as f32, position.y as f32))
            }
            _ => self.input.last_cursor_pos,
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
        runtime: &Runtime,
        window: &dyn KhoraWindow,
    ) {
        // Switch the renderer to offscreen-viewport mode BEFORE
        // `begin_render_frame` runs, so the per-frame `FrameContext`
        // receives the viewport color/depth targets (instead of the
        // swapchain) and agents paint into the texture displayed by the
        // egui viewport panel.
        if let Some(rs_arc) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .cloned()
        {
            if let Ok(mut rs) = rs_arc.lock() {
                rs.set_render_to_viewport(true);
            }
        }

        let Some(overlay_arc) = self.overlay.as_ref() else {
            return;
        };
        let Some(raw_window) = self.raw_window.as_ref() else {
            return;
        };

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

    fn before_agents(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        // First: the scale decides whether this frame's agents integrate at all.
        self.drive_simulation_clock(runtime);

        if let Some(pvfs_arc) = self.project_vfs.as_ref() {
            hot_reload::pump(pvfs_arc, &self.editor_state);
        }

        self.last_view_info = None;

        let Some(rs_arc) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .cloned()
        else {
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
                        // No active scene camera (shouldn't happen
                        // because sync_scene_cameras_for_mode promotes
                        // one) — fall back to the editor cam so the user
                        // still sees something instead of a blank screen.
                        self.camera
                            .lock()
                            .ok()
                            .map(|cam| cam.view_info(vw as f32, vh as f32))
                    })
                    .unwrap_or_default()
            }
        };

        // Publish the active view into the EditorViewportOverride so
        // RenderFlow can fall back to it when no scene Camera is active
        // (Editing mode forces every scene camera inactive). RenderFlow
        // appends the override to RenderWorld.views during its `project`
        // step on the next scheduler tick.
        self.viewport_override
            .set(Some(khora_sdk::khora_data::render::ExtractedView {
                view_proj: view_info.view_projection_matrix(),
                position: view_info.camera_position,
            }));

        // The viewport clear + ground grid are no longer drawn here:
        // the scene pass (RenderAgent) clears the viewport, and the grid
        // is now `GridLane` under `OverlayAgent` (rendered after the
        // scene). The editor only feeds the camera to the renderer.
        wgpu_rs.prepare_frame(&view_info);

        // Toggle the editor ground grid per-frame: it is editor chrome,
        // visible only while editing — hide it in Play / Paused (same
        // gating as the selection gizmos below). The engine ships the
        // `GridConfig` slot disabled; the editor drives it.
        let grid_on = matches!(play_mode, PlayMode::Editing);
        if let Some(grid_cfg) = runtime
            .resources
            .get::<khora_sdk::khora_data::render::SharedGridConfig>()
        {
            if let Ok(mut cfg) = grid_cfg.lock() {
                cfg.enabled = grid_on;
            }
        }

        // Wireframe debug overlay — user-toggled (command palette), and like
        // the grid it is editor chrome, hidden outside Editing mode.
        if let Some(wf_cfg) = runtime
            .resources
            .get::<khora_sdk::khora_data::render::SharedWireframeConfig>()
        {
            if let Ok(mut cfg) = wf_cfg.lock() {
                cfg.enabled = self.wireframe_enabled && grid_on;
            }
        }

        // Collect the current selection gizmos and publish them into the
        // shared `GizmoFrame`. `OverlayAgent`'s `GizmoLane` renders them
        // during the scheduler's OUTPUT phase — the editor is a pure data
        // producer here, the engine owns the rendering mechanism. Gated
        // on `PlayMode::Editing`: gizmos must not bleed into Play / Paused.
        let gizmo_lines = if let Ok(state) = self.editor_state.lock() {
            if state.play_mode != PlayMode::Editing || state.selection.is_empty() {
                Vec::new()
            } else {
                mod_gizmo::collect_gizmo_lines(world, &state, &view_info)
            }
        } else {
            Vec::new()
        };
        if let Some(shared) = runtime
            .resources
            .get::<khora_sdk::khora_data::render::SharedGizmoFrame>()
        {
            if let Ok(mut frame) = shared.lock() {
                frame.lines = gizmo_lines;
            }
        }

        self.last_view_info = Some(view_info);
    }

    fn after_agents(&mut self, _world: &mut GameWorld, runtime: &Runtime) {
        let Some(rs_arc) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
            .cloned()
        else {
            return;
        };

        // Gizmo rendering now happens inside the scheduler via
        // `OverlayAgent`'s `GizmoLane` (the lines are published into the
        // shared `GizmoFrame` from `before_agents`). The editor no longer
        // drives a separate gizmo render pass here.

        // Present the egui overlay last so the dock + panels paint over
        // the 3D scene encoded by the agents.
        let Some(overlay_arc) = self.overlay.as_ref() else {
            return;
        };
        let Some(raw_window) = self.raw_window.as_ref() else {
            return;
        };

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
            // Switch back to swapchain so the egui overlay (dock + panels
            // + viewport panel that displays the offscreen texture)
            // paints onto the presented surface.
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
