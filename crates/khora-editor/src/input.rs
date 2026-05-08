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
//! to the editor camera, gizmo mode switches, command palette, and undo /
//! redo. Viewport-rect aware so dragging across panels does not nudge the
//! camera.

use std::sync::{Arc, Mutex};

use khora_sdk::prelude::*;
use khora_sdk::{CommandHistory, EditorCamera, EditorState, GizmoMode, PlayMode};

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

/// Drive the editor camera + global shortcuts off the per-frame input
/// queue produced by the engine. World access is needed for `Delete`.
pub fn process_events(
    state: &mut InputState,
    inputs: &[InputEvent],
    world: &mut khora_sdk::GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    camera: &Arc<Mutex<EditorCamera>>,
    command_history: &Arc<Mutex<CommandHistory>>,
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
            InputEvent::MouseButtonPressed { button } => match button {
                MouseButton::Middle => state.middle_down = true,
                MouseButton::Right => state.right_down = true,
                _ => {}
            },
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
                if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                    state.shift_held = true;
                }
                if key_code == "ControlLeft" || key_code == "ControlRight" {
                    state.ctrl_held = true;
                }

                if !state.ctrl_held {
                    if let Ok(mut s) = editor_state.lock() {
                        match key_code.as_str() {
                            "KeyQ" => s.gizmo_mode = GizmoMode::Select,
                            "KeyW" => s.gizmo_mode = GizmoMode::Move,
                            "KeyE" => s.gizmo_mode = GizmoMode::Rotate,
                            "KeyR" => s.gizmo_mode = GizmoMode::Scale,
                            _ => {}
                        }
                    }
                }

                if key_code == "Delete" {
                    if let Ok(mut s) = editor_state.lock() {
                        ops::delete_selection(world, &mut s);
                    }
                }

                if key_code == "KeyK" && state.ctrl_held {
                    if let Ok(mut s) = editor_state.lock() {
                        s.command_palette_open = !s.command_palette_open;
                    }
                }

                if key_code == "KeyZ" && state.ctrl_held {
                    if let Ok(mut history) = command_history.lock() {
                        if let Some(edit) = history.undo() {
                            if let Ok(mut s) = editor_state.lock() {
                                s.push_edit(edit);
                            }
                        }
                    }
                }

                if key_code == "KeyY" && state.ctrl_held {
                    if let Ok(mut history) = command_history.lock() {
                        if let Some(edit) = history.redo() {
                            if let Ok(mut s) = editor_state.lock() {
                                s.push_edit(edit);
                            }
                        }
                    }
                }
            }
            InputEvent::KeyReleased { key_code } => {
                if key_code == "ShiftLeft" || key_code == "ShiftRight" {
                    state.shift_held = false;
                }
                if key_code == "ControlLeft" || key_code == "ControlRight" {
                    state.ctrl_held = false;
                }
            }
            InputEvent::MouseMoved { x, y } => {
                if editor_cam_navigable && cursor_in_viewport(*x, *y) {
                    if let Some((px, py)) = state.prev_cursor {
                        let dx = x - px;
                        let dy = y - py;

                        if let Ok(mut cam) = camera.lock() {
                            if state.right_down || (state.middle_down && state.shift_held) {
                                cam.pan(dx, dy);
                            } else if state.middle_down {
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
