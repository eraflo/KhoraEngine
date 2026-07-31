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

use khora_sdk::prelude::*;
use khora_sdk::KeyCode;
use khora_sdk::{EditorCamera, EditorMode, EditorState, GizmoMode, PlayMode};

use crate::ops;

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
    }
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
                    if let Ok(mut s) = editor_state.lock() {
                        match key_code {
                            KeyCode::KeyQ => s.gizmo_mode = GizmoMode::Select,
                            KeyCode::KeyW => s.gizmo_mode = GizmoMode::Move,
                            KeyCode::KeyE => s.gizmo_mode = GizmoMode::Rotate,
                            KeyCode::KeyR => s.gizmo_mode = GizmoMode::Scale,
                            _ => {}
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
