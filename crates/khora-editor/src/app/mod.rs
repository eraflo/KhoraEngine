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
use crate::ops::prefab_overrides::{inspect_against, revert_instance};
use crate::project_vfs::{ProjectPrefabs, ProjectVfs};
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::{instance_of, prefab_world, InstanceOf};

mod engine_app;
mod layout;
mod project_open;

#[cfg(test)]
mod tests;

/// The name of the editor's own engine mode: editing a scene, not playing it.
const EDITOR_MODE: &str = "editor";

/// The engine mode a frame runs in, from where the editor's transport stands.
///
/// Editing is the editor's own mode — not the game, so no script runs. A
/// paused game is still the game: its scripts run, at a delta of zero.
pub(crate) fn engine_mode_for(play_mode: PlayMode) -> khora_sdk::EngineMode {
    match play_mode {
        PlayMode::Editing => khora_sdk::EngineMode::Custom(EDITOR_MODE.to_owned()),
        PlayMode::Playing | PlayMode::Paused => khora_sdk::EngineMode::Playing,
    }
}

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
    /// The inspected entity's instance and its prefab's values, kept while
    /// the selection and the project's files stay the same.
    prefab_view: Option<PrefabView>,
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
            // A duplicated prefab instance stays one: its prefab is read to
            // derive the copy's members from the copy's new root.
            match self.project_vfs.as_deref() {
                Some(pvfs) => ops::duplicate_entity(
                    world,
                    entity,
                    &mut state,
                    &crate::project_vfs::ProjectPrefabs(pvfs),
                ),
                None => ops::duplicate_entity(
                    world,
                    entity,
                    &mut state,
                    &khora_sdk::khora_data::scene::NoPrefabs,
                ),
            }
        }

        if let Some((entity, type_name)) = state.pending_add_component.take() {
            ops::add_component_to_entity(world, entity, &type_name);
        }

        if let Some(entity) = state.pending_prefab_revert.take() {
            revert_prefab_instance(self.project_vfs.as_deref(), world, entity, &mut state);
        }

        ops::extract_scene_tree(world, &mut state);
        ops::extract_inspected(world, &mut state);
        attach_prefab_view(
            &mut self.prefab_view,
            self.project_vfs.as_deref(),
            world,
            &mut state,
        );

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

/// What decides which instance an entity belongs to: the entity, the project
/// files' generation, and its ancestors with the prefab each links to — a
/// reparent, a root deleted or unlinked, a prefab rewritten all change it.
#[derive(Clone, PartialEq)]
struct ViewKey {
    entity: EntityId,
    generation: u64,
    ancestry: Vec<(EntityId, Option<khora_sdk::khora_core::asset::AssetUUID>)>,
}

/// `entity` and its ancestors, each with the prefab it links to.
fn ancestry(
    world: &GameWorld,
    entity: EntityId,
) -> Vec<(EntityId, Option<khora_sdk::khora_core::asset::AssetUUID>)> {
    let inner = world.inner_world();
    let mut chain = Vec::new();
    let mut current = Some(entity);
    while let Some(at) = current {
        if chain.iter().any(|(seen, _)| *seen == at) {
            break;
        }
        let link = inner
            .get::<khora_sdk::khora_data::ecs::PrefabInstance>(at)
            .map(|link| link.prefab);
        chain.push((at, link));
        current = inner
            .get::<khora_sdk::khora_data::ecs::Parent>(at)
            .map(|parent| parent.0)
            .filter(|parent| inner.contains(*parent));
    }
    chain
}

/// The inspected entity's instance and its prefab expanded under the
/// instance root, for one selection under one generation of the project's
/// files.
struct PrefabView {
    key: ViewKey,
    found: Option<(InstanceOf, World)>,
    path: Option<String>,
}

/// Sees the inspected entity against its instance's prefab, if it belongs
/// to one. The prefab is read once per selection and per generation of
/// the project's files; the comparison runs every frame, so an edit shows
/// its override at once.
fn attach_prefab_view(
    view: &mut Option<PrefabView>,
    project_vfs: Option<&Mutex<ProjectVfs>>,
    world: &GameWorld,
    state: &mut EditorState,
) {
    let Some(inspected) = state.inspected.as_mut() else {
        *view = None;
        return;
    };
    let Some(pvfs) = project_vfs else {
        return;
    };
    let generation = pvfs.lock().map(|vfs| vfs.generation()).unwrap_or(0);
    let key = ViewKey {
        entity: inspected.entity,
        generation,
        ancestry: ancestry(world, inspected.entity),
    };
    if view.as_ref().map(|view| &view.key) != Some(&key) {
        let prefabs = ProjectPrefabs(pvfs);
        let inner = world.inner_world();
        let found = instance_of(inner, inspected.entity, &prefabs).and_then(|instance| {
            prefab_world(inner, &instance, &prefabs)
                .ok()
                .map(|scratch| (instance, scratch))
        });
        let path = found
            .as_ref()
            .and_then(|(instance, _)| pvfs.lock().ok()?.rel_path_of(instance.prefab));
        *view = Some(PrefabView { key, found, path });
    }
    let Some(PrefabView {
        found: Some((instance, scratch)),
        path,
        ..
    }) = view.as_ref()
    else {
        return;
    };
    inspected.prefab = inspect_against(world.inner_world(), inspected.entity, instance, scratch)
        .map(|mut prefab| {
            prefab.prefab_path = path.clone();
            prefab
        });
}

/// Takes the instance `entity` belongs to back to its prefab's values.
fn revert_prefab_instance(
    project_vfs: Option<&Mutex<ProjectVfs>>,
    world: &GameWorld,
    entity: EntityId,
    state: &mut EditorState,
) {
    let Some(pvfs) = project_vfs else {
        return;
    };
    let prefabs = ProjectPrefabs(pvfs);
    let inner = world.inner_world();
    let Some(instance) = instance_of(inner, entity, &prefabs) else {
        return;
    };
    match prefab_world(inner, &instance, &prefabs) {
        Ok(scratch) => {
            for edit in revert_instance(inner, &instance, &scratch) {
                state.push_edit(edit);
            }
        }
        Err(e) => log::error!("Failed to read the prefab to revert to: {e}"),
    }
}

impl EditorApp {
    /// Tells the engine how fast the simulated world should run.
    ///
    /// One of the two things `PlayMode` causes to cross into the engine — the
    /// engine mode, which decides whether scripts run, is the other. The scale
    /// crosses as a number: `0.0` is what a pause menu or a cutscene sets too,
    /// so the editor is one caller among several rather than a special case the
    /// engine has to know about.
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

    /// Tells the engine which mode this frame runs in: the editor's own while
    /// editing — no script runs, the scene is the author's — and the game's
    /// once Play is pressed. Written before the agents, so the whole frame
    /// runs in one mode.
    fn drive_engine_mode(&self, runtime: &Runtime) {
        let Some(shared) = runtime.resources.get::<khora_sdk::SharedEngineMode>() else {
            return;
        };
        let play_mode = self
            .editor_state
            .lock()
            .ok()
            .map(|state| state.play_mode)
            .unwrap_or(PlayMode::Editing);
        let mode = engine_mode_for(play_mode);
        if let Ok(mut current) = shared.write() {
            if *current != mode {
                *current = mode;
            }
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
