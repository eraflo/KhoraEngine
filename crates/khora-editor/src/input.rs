// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Editor input dispatch — keyboard shortcuts + camera navigation.
//!
//! Pulls per-frame `InputEvent`s from `EditorApp::update` and routes them
//! to the editor camera, gizmo mode switches, the command palette and the
//! workspace shortcuts. Viewport-rect aware so dragging across panels does not
//! nudge the camera.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{pick_handle, GizmoDrag, GizmoTransform};
use khora_sdk::khora_core::math::Ray;
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::prelude::*;
use khora_sdk::KeyCode;
use khora_sdk::{EditorCamera, EditorMode, EditorState, GizmoMode, PlayMode};

use crate::{mod_gizmo, ops};

/// State of modifier keys + button drags that has to outlive a single
/// frame. Lives on `EditorApp` and is mutated through this module.
#[derive(Default)]
pub struct InputState {
    pub middle_down: bool,
    pub right_down: bool,
    pub shift_held: bool,
    pub ctrl_held: bool,
    pub prev_cursor: Option<(f32, f32)>,
    /// Last known cursor position in physical screen pixels — kept in
    /// sync with every `WindowEvent::CursorMoved`. Used by
    /// `intercept_window_event` to test whether a `MouseInput` event
    /// (which carries no position) lands inside the 3D viewport rect.
    pub last_cursor_pos: Option<(f32, f32)>,
    /// The gizmo handle currently being dragged, if any.
    pub gizmo_drag: Option<GizmoDrag>,
    /// Transforms the selection had when that drag began. A drag resolves
    /// against these rather than against the live values, so it cannot
    /// accumulate drift over a long gesture.
    pub gizmo_starts: Vec<(EntityId, GizmoTransform)>,
}

impl InputState {
    /// Drops every held button and modifier.
    ///
    /// Called when the window loses focus: a release that happens while another
    /// application is in front never reaches us, so without this the flag stays
    /// set forever — Alt-Tab with the middle button down and the camera orbits
    /// on plain mouse movement from then on. A stuck `ctrl_held` is quieter and
    /// worse: it disables the gizmo shortcuts, which just stop working.
    pub fn release_all(&mut self) {
        self.middle_down = false;
        self.right_down = false;
        self.shift_held = false;
        self.ctrl_held = false;
        self.prev_cursor = None;
        self.end_gizmo_drag();
    }

    /// Drops any manipulation in progress, leaving the entities where the last
    /// resolved delta put them.
    fn end_gizmo_drag(&mut self) {
        self.gizmo_drag = None;
        self.gizmo_starts.clear();
    }
}

/// The cursor ray for a screen position, or `None` when the viewport has not
/// been laid out yet.
fn cursor_ray(
    camera: &Arc<Mutex<EditorCamera>>,
    cursor: Option<(f32, f32)>,
    viewport: Option<[f32; 4]>,
) -> Option<Ray> {
    let (cx, cy) = cursor?;
    let [rx, ry, rw, rh] = viewport?;
    let camera = camera.lock().ok()?;
    Some(camera.screen_to_ray(cx - rx, cy - ry, rw, rh))
}

/// Drive the editor camera + global shortcuts off the per-frame input
/// queue produced by the engine. World access is needed for `Delete`.
pub fn process_events(
    state: &mut InputState,
    inputs: &[InputEvent],
    world: &mut khora_sdk::GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    camera: &Arc<Mutex<EditorCamera>>,
) {
    let (viewport_rect, play_mode) = editor_state
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
            InputEvent::MouseButtonPressed { button } => {
                // Only arm camera navigation when the press *starts* in the
                // viewport. Pressing on the inspector and dragging across used
                // to grab the camera halfway.
                let started_in_viewport = state
                    .last_cursor_pos
                    .map(|(x, y)| cursor_in_viewport(x, y))
                    .unwrap_or(false);
                if started_in_viewport {
                    match button {
                        MouseButton::Middle => state.middle_down = true,
                        MouseButton::Right => state.right_down = true,
                        // Left click grabs a gizmo handle, or picks. Selection
                        // is no longer hierarchy-only: `GizmoMode::Select` and
                        // `screen_to_ray` both existed, but nothing cast a ray
                        // against the scene, so the viewport was read-only.
                        MouseButton::Left => {
                            let ray = cursor_ray(camera, state.last_cursor_pos, viewport_rect);
                            let view = viewport_rect.and_then(|[_, _, rw, rh]| {
                                camera.lock().ok().map(|cam| cam.view_info(rw, rh))
                            });

                            if let (Some(ray), Some(view)) = (ray, view) {
                                // A handle under the cursor wins over whatever
                                // is behind it: the manipulator sits on top of
                                // its own object, so picking first would make
                                // it impossible to grab.
                                let grabbed = editor_state.lock().ok().and_then(|s| {
                                    let frame = mod_gizmo::selection_frame(world, &s, &view)?;
                                    let axis = pick_handle(
                                        s.gizmo_mode,
                                        frame.pivot,
                                        &frame.basis,
                                        frame.size,
                                        &ray,
                                    )?;
                                    let drag = GizmoDrag::begin(
                                        s.gizmo_mode,
                                        axis,
                                        frame.pivot,
                                        &frame.basis,
                                        frame.size,
                                        &ray,
                                    )?;
                                    Some((drag, mod_gizmo::capture_starts(world, &s)))
                                });

                                if let Some((drag, starts)) = grabbed {
                                    state.gizmo_drag = Some(drag);
                                    state.gizmo_starts = starts;
                                } else if let Ok(mut s) = editor_state.lock() {
                                    match crate::picking::pick_entity(world, &ray) {
                                        // Ctrl extends the selection, the
                                        // same modifier the hierarchy uses.
                                        Some(entity) if state.ctrl_held => s.toggle_select(entity),
                                        Some(entity) => s.select(entity),
                                        // Clicking empty space deselects —
                                        // otherwise there is no way to let
                                        // go of a selection in the viewport.
                                        None if !state.ctrl_held => {
                                            s.clear_selection();
                                            s.inspected = None;
                                        }
                                        None => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            InputEvent::MouseButtonReleased { button } => match button {
                MouseButton::Middle => {
                    state.middle_down = false;
                    state.prev_cursor = None;
                }
                MouseButton::Right => {
                    state.right_down = false;
                    state.prev_cursor = None;
                }
                MouseButton::Left => state.end_gizmo_drag(),
                _ => {}
            },
            InputEvent::KeyPressed { key_code } => {
                if matches!(key_code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    state.shift_held = true;
                }
                if matches!(key_code, KeyCode::ControlLeft | KeyCode::ControlRight) {
                    state.ctrl_held = true;
                }

                if !state.ctrl_held {
                    let tool = match key_code {
                        KeyCode::KeyQ => Some(GizmoMode::Select),
                        KeyCode::KeyW => Some(GizmoMode::Move),
                        KeyCode::KeyE => Some(GizmoMode::Rotate),
                        KeyCode::KeyR => Some(GizmoMode::Scale),
                        _ => None,
                    };
                    if let Some(tool) = tool {
                        // A drag belongs to the tool it started with — its
                        // grabbed parameter means nothing under another one.
                        state.end_gizmo_drag();
                        if let Ok(mut s) = editor_state.lock() {
                            s.gizmo_mode = tool;
                        }
                    }
                }

                if *key_code == KeyCode::Delete {
                    if let Ok(mut s) = editor_state.lock() {
                        ops::delete_selection(world, &mut s);
                    }
                }

                if *key_code == KeyCode::KeyK && state.ctrl_held {
                    if let Ok(mut s) = editor_state.lock() {
                        s.command_palette_open = !s.command_palette_open;
                    }
                }

                // Ctrl+S — the shortcut every editor has, and the one whose
                // absence people discover by losing work.
                if *key_code == KeyCode::KeyS && state.ctrl_held {
                    if let Ok(mut s) = editor_state.lock() {
                        s.pending_menu_action = Some("save".to_owned());
                    }
                }

                // Ctrl+1 / Ctrl+2 — workspace switching, as the design doc
                // specifies. Plain digits stay free for future tool bindings.
                if state.ctrl_held {
                    let mode = match key_code {
                        KeyCode::Digit1 => Some(EditorMode::Scene),
                        KeyCode::Digit2 => Some(EditorMode::ControlPlane),
                        _ => None,
                    };
                    if let Some(mode) = mode {
                        if let Ok(mut s) = editor_state.lock() {
                            s.active_mode = mode;
                        }
                    }
                }

                // Escape retreats one level. Today that means closing the
                // palette, then clearing the selection — the design doc's
                // "Esc always retreats" applied to what exists.
                if *key_code == KeyCode::Escape {
                    if let Ok(mut s) = editor_state.lock() {
                        if s.command_palette_open {
                            s.command_palette_open = false;
                        } else if !s.selection.is_empty() {
                            s.clear_selection();
                            s.inspected = None;
                        }
                    }
                }

                // Ctrl+Z / Ctrl+Y are unbound on purpose: nothing pushes onto
                // `CommandHistory`, so both were no-ops. A shortcut that
                // silently does nothing is worse than an absent one — it
                // teaches the user their edits are reversible when they are not.
            }
            InputEvent::KeyReleased { key_code } => {
                if matches!(key_code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    state.shift_held = false;
                }
                if matches!(key_code, KeyCode::ControlLeft | KeyCode::ControlRight) {
                    state.ctrl_held = false;
                }
            }
            InputEvent::MouseMoved { x, y } => {
                // A manipulation in progress owns the pointer: the camera must
                // not also move, or the object being dragged slides out from
                // under the cursor.
                if state.gizmo_drag.is_some() {
                    if let Some(ray) = cursor_ray(camera, Some((*x, *y)), viewport_rect) {
                        let delta = state.gizmo_drag.as_mut().and_then(|d| d.update(&ray));
                        if let Some(delta) = delta {
                            mod_gizmo::apply_delta(world, delta, &state.gizmo_starts);
                        }
                    }
                    state.prev_cursor = Some((*x, *y));
                    continue;
                }

                if editor_cam_navigable && cursor_in_viewport(*x, *y) {
                    if let Some((px, py)) = state.prev_cursor {
                        let dx = x - px;
                        let dy = y - py;

                        if let Ok(mut cam) = camera.lock() {
                            // DCC convention, shared by Blender, Unity, Unreal
                            // and Godot: middle orbits, shift+middle pans.
                            // Right-drag used to pan, which left the button
                            // doing something no other 3D tool does and wasted
                            // the one people reach for to look around.
                            if state.middle_down && state.shift_held {
                                cam.pan(dx, dy);
                            } else if state.middle_down || state.right_down {
                                cam.orbit(dx, dy);
                            }
                        }
                    }
                }
                state.prev_cursor = Some((*x, *y));
            }
            InputEvent::MouseWheelScrolled { delta_y, .. } => {
                let in_view = state
                    .last_cursor_pos
                    .map(|(x, y)| cursor_in_viewport(x, y))
                    .unwrap_or(false);
                if editor_cam_navigable && in_view {
                    if let Ok(mut cam) = camera.lock() {
                        cam.zoom(*delta_y);
                    }
                }
            }
        }
    }
}
