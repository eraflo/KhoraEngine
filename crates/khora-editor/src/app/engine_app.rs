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
use khora_sdk::{EditorState, PlayMode};
use khora_sdk::{EngineApp, GameWorld, InputEvent, Runtime};

use super::EditorApp;
use crate::bootstrap::load_logo_icon;
use crate::camera::EditorCamera;
use crate::commands::CommandHistory;
use crate::input::InputState;
use crate::log_capture::EditorLogCapture;
use crate::{commands, gizmo, hot_reload, input, ops};

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
            loading: None,
            brand_installed: false,
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
            asset_watch: Arc::new(khora_sdk::AssetWatch::default()),
            mounted_scripts: None,
            opening: None,
            startup_hold: None,
            problems: None,
            script_catalog: None,
            prefab_view: None,
            play_prefabs: Arc::default(),
            prefab_catalog: khora_sdk::khora_data::scene::PrefabCatalog::new(),
            shared_assets: Arc::new(Mutex::new(khora_sdk::AssetService::empty())),
            scene_loads: None,
        }
    }

    /// What the engine reads of the editor: its state, its camera, the
    /// viewport override, the open project's prefabs and assets.
    fn provide(&self, runtime: &mut Runtime) {
        runtime.resources.insert(self.editor_state.clone());
        runtime.resources.insert(self.camera.clone());
        // Editor viewport override — RenderFlow reads it as fallback
        // when no scene Camera is active (Editing mode).
        runtime.resources.insert(self.viewport_override.clone());
        // What a script's prefab `Spawn` reads: the open project's prefabs,
        // and the catalog its turn is checked against.
        runtime
            .resources
            .insert(khora_sdk::khora_data::scene::PrefabLibrary(
                self.play_prefabs.clone(),
            ));
        runtime.resources.insert(self.prefab_catalog.clone());
        // What the engine's asset resolver loads the scene's meshes,
        // materials and textures through: the open project's assets.
        runtime.services.insert(Arc::clone(&self.shared_assets));
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        // Editing, not playing: the scene's scripts, physics and sounds run
        // only once Play is pressed.
        commands::start_editing(runtime);
        self.cache_services(runtime);
        // The loading film, over the window from the first frame on: the
        // frame hooks paint it, and it goes once the startup is over.
        self.begin_startup(runtime);
        self.register_panels(runtime);
        // The `--project`, opened behind the film: its index, then its scene
        // and its scripts, filed with the engine's families by ticket.
        if let Some(Some(project)) = crate::bootstrap::PROJECT_PATH.get() {
            let root = std::path::PathBuf::from(project);
            if root.exists() {
                self.opening = super::project_open::start_opening_project(
                    root,
                    runtime,
                    &self.editor_state,
                    &mut self.startup_hold,
                );
            } else {
                log::warn!("--project path does not exist: {project}");
            }
        }
        let _ = world;
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
            match &self.scene_loads {
                Some(loads) => {
                    commands::open_scene_dispatch(
                        self.project_vfs.as_ref(),
                        world,
                        &self.editor_state,
                        loads,
                        std::path::Path::new(&path),
                    );
                }
                None => log::error!("cannot open '{path}': the engine holds no scene load hub"),
            }
        }

        // Until Stop's restore lands, the world is still the last play's: an
        // edit made to it now would be undone by the restore. Edits wait, and
        // those already made of it are refused.
        let restoring = match self.editor_state.lock() {
            Ok(mut state) if state.restoring.is_some() => {
                let refused = state.drain_edits().len();
                if refused > 0 {
                    log::info!("{refused} edit(s) refused while the scene is being restored");
                }
                true
            }
            _ => false,
        };
        if restoring {
            self.tick_state(world);
            return;
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
            &self.asset_watch,
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
        commands::process_pending_prefab_apply(
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

        // While the loading screen covers the editor, it is the only thing on
        // screen: egui has the event (a click or Esc there skips the film),
        // and nothing under the cover — the viewport's picking and camera,
        // the editor's shortcuts — gets it.
        if self.loading.is_some() {
            return true;
        }

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

    fn startup_ready(&mut self) {
        self.end_startup();
    }

    fn before_frame(&mut self, world: &mut GameWorld, runtime: &Runtime, window: &dyn KhoraWindow) {
        self.show_frame(runtime, window);
        // The shell has been shown, so a click on Play this frame is pending:
        // the transport is processed now, before the engine fixes the frame,
        // so the frame is wholly edited or wholly played.
        // What landed at the last boundary first (an opened scene, Stop's
        // restore), so the transport sees whether a restore is under way.
        if let Some(loads) = &self.scene_loads {
            if commands::land_scene_loads(world, &self.editor_state, loads) {
                commands::forget_history(&self.command_history);
            }
        }
        // The `--project`, once its index landed: its VFS over the engine's
        // assets, its default scene filed.
        // A project the user opened meanwhile — the film gave up waiting —
        // has the last word: the command line's lands nothing over it.
        if self.opening.is_some() && self.project_vfs.is_some() {
            log::warn!("the --project open is dropped: another project was opened meanwhile");
            self.opening = None;
            self.startup_hold = None;
        }
        if self.opening.is_some() {
            if let Some(pvfs) = super::project_open::land_opened_project(
                &mut self.opening,
                world,
                runtime,
                &self.shared_assets,
                &self.asset_watch,
                &self.editor_state,
                &mut self.startup_hold,
            ) {
                self.project_vfs = Some(pvfs);
            }
        }
        if let Some(pvfs) = self.project_vfs.as_ref() {
            let actions = hot_reload::land_reindex(pvfs, &self.editor_state);
            commands::apply_after_index(world, actions);
        }
        commands::process_transport(world, &self.editor_state, runtime);
    }

    fn before_agents(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        if let Some(pvfs_arc) = self.project_vfs.as_ref() {
            // The project's reindexes are filed with the engine's hub.
            if let (Ok(mut pvfs), Some(indexes)) = (
                pvfs_arc.lock(),
                runtime.resources.get::<Arc<khora_sdk::IndexHub>>(),
            ) {
                pvfs.index_with(Arc::clone(indexes));
            }
            hot_reload::pump(pvfs_arc, &self.editor_state);
        }
        // The open project's scripts: mounted as it opens, swapped as another
        // opens — what they declare registers while editing, what they
        // compiled to runs at Play.
        let project = self
            .project_vfs
            .as_ref()
            .and_then(|pvfs| pvfs.lock().ok().map(|pvfs| pvfs.root.clone()));
        super::project_open::mount_opened_scripts(
            runtime,
            &mut self.mounted_scripts,
            project.as_deref(),
            &mut self.startup_hold,
        );
        // And its prefabs: what a game in play spawns, and what its `Spawn`
        // is checked against.
        self.play_prefabs
            .follow(self.project_vfs.as_ref(), &self.prefab_catalog);
        // And its assets: the engine resolves the scene's references through
        // them. A project opened since the last frame moves in here.
        if let Some(Ok(mut pvfs)) = self.project_vfs.as_ref().map(|pvfs| pvfs.lock()) {
            pvfs.share_assets(&self.shared_assets);
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
        // The shape every camera in this viewport takes — the game camera's
        // in Play, and the one a selected camera's frustum is drawn at.
        let viewport_aspect = {
            use khora_sdk::khora_core::lane::ColorTargetSize;
            use khora_sdk::khora_core::math::Extent2D;
            ColorTargetSize(Extent2D {
                width: vw,
                height: vh,
            })
            .aspect()
        };
        let play_mode = self
            .editor_state
            .lock()
            .ok()
            .map(|s| s.play_mode)
            .unwrap_or(PlayMode::Editing);

        let mut broken_play_camera = None;
        let view_info = match play_mode {
            PlayMode::Editing => match self.camera.lock() {
                Ok(cam) => cam.view_info(vw as f32, vh as f32),
                Err(_) => ViewInfo::default(),
            },
            PlayMode::Playing | PlayMode::Paused => {
                use khora_sdk::khora_data::render::extract_active_camera_view;
                // The game camera takes the shape of the viewport it fills.
                match extract_active_camera_view(world.inner_world(), viewport_aspect) {
                    Some(Ok(view)) => view,
                    // The active camera's values make no image: the game
                    // renders nothing, and the editor camera does not stand
                    // in for it (the flows ignore the override while a scene
                    // camera is active).
                    Some(Err(problem)) => {
                        // Named over the viewport, so the empty picture says why.
                        broken_play_camera = world
                            .inner_world()
                            .query::<(
                                khora_sdk::prelude::ecs::EntityId,
                                &khora_sdk::prelude::ecs::Camera,
                            )>()
                            .find(|(_, camera)| camera.is_active)
                            .map(|(entity, _)| (entity, problem.to_string()));
                        ViewInfo::default()
                    }
                    // No active scene camera (shouldn't happen because
                    // sync_scene_cameras_for_mode promotes one) — fall back
                    // to the editor cam so the user still sees something
                    // instead of a blank screen.
                    None => self
                        .camera
                        .lock()
                        .ok()
                        .map(|cam| cam.view_info(vw as f32, vh as f32))
                        .unwrap_or_default(),
                }
            }
        };

        if let Ok(mut state) = self.editor_state.lock() {
            if state.broken_play_camera != broken_play_camera {
                state.broken_play_camera = broken_play_camera;
            }
        }

        // Publish the viewport camera into the EditorViewportOverride: the
        // flows fall back to it when no scene Camera is active (Editing mode
        // forces every scene camera inactive), and the engine prepares the
        // renderer with it after the flows run, as it would with a scene
        // camera. The editor never prepares the renderer itself.
        self.viewport_override.set_camera(Some(view_info.clone()));

        // Toggle the editor ground grid per-frame: it is editor chrome,
        // visible only while editing — hide it in Play / Paused (same
        // gating as the selection gizmos below). The engine ships the
        // `GridConfig` slot disabled; the editor drives it.
        let grid_on = matches!(play_mode, PlayMode::Editing);
        let host_views = runtime
            .resources
            .get::<Arc<khora_sdk::khora_core::lane::HostViews>>()
            .cloned();
        if let Some(host_views) = &host_views {
            host_views.set(khora_sdk::khora_data::render::GridView(
                khora_sdk::khora_data::render::GridConfig { enabled: grid_on },
            ));
            // Wireframe debug overlay — user-toggled (command palette), and
            // like the grid it is editor chrome, hidden outside Editing mode.
            host_views.set(khora_sdk::khora_data::render::WireframeView(
                khora_sdk::khora_data::render::WireframeConfig {
                    enabled: self.wireframe_enabled && grid_on,
                    ..Default::default()
                },
            ));
        }

        // Collect the current selection gizmos and set them as the gizmo
        // View. The gizmo family's `GizmoLane` renders them
        // during the scheduler's OUTPUT phase — the editor is a pure data
        // producer here, the engine owns the rendering mechanism. Gated
        // on `PlayMode::Editing`: gizmos must not bleed into Play / Paused.
        let gizmo_lines = if let Ok(state) = self.editor_state.lock() {
            if state.play_mode != PlayMode::Editing || state.selection.is_empty() {
                Vec::new()
            } else {
                gizmo::collect_gizmo_lines(world, &state, &view_info, viewport_aspect)
            }
        } else {
            Vec::new()
        };
        if let Some(host_views) = &host_views {
            host_views.set(khora_sdk::khora_data::render::GizmoView(
                khora_sdk::khora_data::render::GizmoFrame { lines: gizmo_lines },
            ));
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
        // the gizmo family's `GizmoLane` (the lines are published into the
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

impl EditorApp {
    /// The editor's shell for this frame: the renderer switched to the
    /// viewport, the overlay begun, the panels shown and the viewport target
    /// sized from its panel.
    fn show_frame(&mut self, runtime: &Runtime, window: &dyn KhoraWindow) {
        self.drop_finished_loading_screen();
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

        // The panel is laid out: the viewport target takes its shape, so the
        // cameras projected this frame read the panel's real aspect. A hidden
        // panel asks for nothing and the target keeps its last size.
        let viewport_rect = self
            .editor_state
            .lock()
            .ok()
            .and_then(|s| s.viewport_screen_rect);
        if let (Some(rect), Some(rs_arc)) = (
            viewport_rect,
            runtime
                .backends
                .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
                .cloned(),
        ) {
            if let Ok(mut rs) = rs_arc.lock() {
                if let Some(wgpu_rs) = rs.as_any_mut().downcast_mut::<WgpuRenderSystem>() {
                    let (width, height) = crate::panels::viewport::target_size_for(rect);
                    wgpu_rs.request_viewport_size(width, height);
                }
            }
        }
    }
}
