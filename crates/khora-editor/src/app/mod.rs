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

//! Editor application root.
//!
//! Implements `EngineApp`, `AgentProvider`, `PhaseProvider`. Per-frame
//! work is delegated to the focused modules (`input`, `commands`,
//! `hot_reload`) so this file stays a composition layer rather than a
//! 1000-line god struct.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
use khora_sdk::khora_core::ui::EditorOverlay;
use khora_sdk::prelude::ecs::*;
use khora_sdk::winit;
use khora_sdk::{AgentProvider, GameWorld, Runtime};
use khora_sdk::{DccService, PlayMode};
use khora_sdk::{EditorShell, EditorState, LogEntry};

use crate::camera::EditorCamera;
use crate::commands::CommandHistory;

use crate::input::InputState;
use crate::ops;
use crate::project_vfs::ProjectVfs;

mod engine_app;
mod layout;
mod project_open;

pub struct EditorApp {
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
    /// `setup` from the ServiceRegistry; queried each frame to push
    /// CPU/GPU load, VRAM, draw calls and triangles into
    /// `EditorState::status`.
    monitors: Option<khora_sdk::MonitorRegistry>,
    /// Live agent registry (Phase 2.3). Used by the Control Plane
    /// workspace to enumerate agents instead of the previous mock list.
    agent_registry: Option<Arc<Mutex<khora_sdk::AgentRegistry>>>,
    /// DCC context handle (Phase 2.2). Locked each frame for the Control
    /// Plane summary bar — exposes mode, budget multiplier, hardware
    /// load.
    dcc_context: Option<Arc<std::sync::RwLock<khora_sdk::DccContext>>>,
    /// View info computed in `before_agents` and re-used by
    /// `after_agents` for gizmo rendering.
    last_view_info: Option<ViewInfo>,
    input: InputState,
    /// Whether the wireframe debug overlay is on. Toggled from the command
    /// palette ("Toggle Wireframe") and pushed into the shared
    /// `WireframeConfig` each frame in `before_agents`.
    wireframe_enabled: bool,
    last_frame_time: Instant,
    /// Editor viewport override — read by `RenderFlow` as fallback when
    /// no active scene `Camera` exists (i.e. Editing mode). Updated each
    /// frame in `before_agents` from the editor's free camera.
    viewport_override: khora_sdk::khora_data::render::EditorViewportOverride,
    /// Project-scoped VFS — instantiated in `setup` once the project
    /// path resolves. All scene I/O, asset enumeration and hot-reload
    /// routes through this. `None` when the editor was launched without
    /// a project (rare; the hub always passes `--project`).
    project_vfs: Option<Arc<Mutex<ProjectVfs>>>,
}

impl EditorApp {
    fn cache_services(&mut self, runtime: &Runtime) {
        if let Some(camera) = runtime
            .resources
            .get::<std::sync::Arc<std::sync::Mutex<EditorCamera>>>()
            .cloned()
        {
            self.camera = camera;
        }
        self.overlay = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn EditorOverlay>>>>()
            .cloned();
        self.monitors = runtime
            .resources
            .get::<khora_sdk::MonitorRegistry>()
            .cloned();
        self.agent_registry = runtime
            .resources
            .get::<Arc<Mutex<khora_sdk::AgentRegistry>>>()
            .cloned();
        self.dcc_context = runtime
            .resources
            .get::<Arc<std::sync::RwLock<khora_sdk::DccContext>>>()
            .cloned();
        self.raw_window = runtime
            .resources
            .get::<Arc<winit::window::Window>>()
            .cloned();
    }

    fn tick_state(&mut self, world: &mut GameWorld) {
        let mut state = match self.editor_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        state.ctrl_held = self.input.ctrl_held;

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
                        state.status.memory_used_mb = r.current_bytes as f32 / (1024.0 * 1024.0);
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

impl EditorApp {
    /// Tells the engine how fast the simulated world should run.
    ///
    /// The **only** thing `PlayMode` causes to cross into the engine, and it
    /// crosses as a number rather than as a mode: a scale of `0.0` is what a
    /// pause menu or a cutscene sets too, so the editor is one caller among
    /// several rather than a special case the engine has to know about.
    ///
    /// Before this, a rigid body fell while nobody had pressed Play — the
    /// engine had no way to be told the world was not meant to be running, and
    /// the transport pill's claim to "tell the truth about what the engine is
    /// doing" was decoration.
    fn drive_simulation_clock(&self, runtime: &Runtime) {
        let Some(shared) = runtime.resources.get::<khora_sdk::prelude::SharedTime>() else {
            return;
        };
        let play_mode = self
            .editor_state
            .lock()
            .ok()
            .map(|state| state.play_mode)
            .unwrap_or(PlayMode::Editing);

        // `Paused` stops it for the same reason `Editing` does: the author is
        // looking at a frozen world in both, and the difference between them is
        // what the editor draws, not what the simulation does.
        let scale = match play_mode {
            PlayMode::Playing => 1.0,
            PlayMode::Editing | PlayMode::Paused => 0.0,
        };
        if let Ok(mut time) = shared.write() {
            time.set_scale(scale);
        }
    }
}

impl AgentProvider for EditorApp {
    fn register_agents(&self, _dcc: &DccService, runtime: &mut Runtime) {
        runtime.resources.insert(self.editor_state.clone());
        runtime.resources.insert(self.camera.clone());
        // Editor viewport override — RenderFlow reads it as fallback
        // when no scene Camera is active (Editing mode).
        runtime.resources.insert(self.viewport_override.clone());
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
