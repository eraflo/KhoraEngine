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

//! The fly-camera player: input bindings and movement.

use khora_sdk::khora_core::platform::{InputBinding, InputMap};
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::{InputEvent, KeyCode};

// Action names used by the player controller. Centralised so both the
// `bind` calls and the `is_pressed` queries point at the same strings.
const ACTION_FORWARD: &str = "player.forward";

const ACTION_BACKWARD: &str = "player.backward";

const ACTION_LEFT: &str = "player.left";

const ACTION_RIGHT: &str = "player.right";

const ACTION_UP: &str = "player.up";

const ACTION_DOWN: &str = "player.down";

/// Simple camera controller for the player.
///
/// Movement is driven through the engine-wide [`InputMap`] (action /
/// binding map). Mouse look stays on raw [`InputEvent`]s because mouse
/// motion isn't representable as an action — the InputMap is for boolean
/// inputs (held / just-pressed / just-released).
pub(super) struct PlayerController {
    pub(super) speed: f32,
    pub(super) sensitivity: f32,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) mouse_captured: bool,
    pub(super) last_mouse: (f32, f32),
}

impl PlayerController {
    pub(super) fn new() -> Self {
        Self {
            speed: 5.0,
            sensitivity: 0.003,
            yaw: std::f32::consts::PI,
            pitch: 0.0,
            mouse_captured: false,
            last_mouse: (0.0, 0.0),
        }
    }

    /// Registers the controller's action bindings with the engine
    /// [`InputMap`]. Called once at setup. Multiple bindings per action
    /// (e.g. arrows AND WASD) demonstrate the OR semantics.
    pub(super) fn bind_actions(map: &mut InputMap) {
        map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::KeyW));
        map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::ArrowUp));
        map.bind(ACTION_BACKWARD, InputBinding::Key(KeyCode::KeyS));
        map.bind(ACTION_BACKWARD, InputBinding::Key(KeyCode::ArrowDown));
        map.bind(ACTION_LEFT, InputBinding::Key(KeyCode::KeyA));
        map.bind(ACTION_LEFT, InputBinding::Key(KeyCode::ArrowLeft));
        map.bind(ACTION_RIGHT, InputBinding::Key(KeyCode::KeyD));
        map.bind(ACTION_RIGHT, InputBinding::Key(KeyCode::ArrowRight));
        map.bind(ACTION_UP, InputBinding::Key(KeyCode::Space));
        map.bind(ACTION_DOWN, InputBinding::Key(KeyCode::ShiftLeft));
        map.bind(ACTION_DOWN, InputBinding::Key(KeyCode::ShiftRight));
    }

    /// Drives mouse look from raw events. Movement keys are queried from
    /// the engine's [`InputMap`] in [`Self::movement_axes`] so this loop
    /// only handles things the InputMap can't model.
    pub(super) fn process_input(&mut self, inputs: &[InputEvent]) {
        use khora_sdk::prelude::MouseButton;

        for event in inputs {
            match event {
                InputEvent::MouseButtonPressed { button } if *button == MouseButton::Right => {
                    self.mouse_captured = true;
                }
                InputEvent::MouseButtonReleased { button } if *button == MouseButton::Right => {
                    self.mouse_captured = false;
                }
                InputEvent::MouseMoved { x, y } => {
                    if self.mouse_captured {
                        let dx = x - self.last_mouse.0;
                        let dy = y - self.last_mouse.1;

                        self.yaw -= dx * self.sensitivity;
                        self.pitch -= dy * self.sensitivity;
                        self.pitch = self.pitch.clamp(
                            -std::f32::consts::FRAC_PI_2 + 0.01,
                            std::f32::consts::FRAC_PI_2 - 0.01,
                        );
                    }
                    self.last_mouse = (*x, *y);
                }
                _ => {}
            }
        }
    }

    /// Resolves the current movement axes from the [`InputMap`].
    /// Returns `(forward, right, up)` in {-1, 0, 1} per axis.
    fn movement_axes(input_map: &InputMap) -> (f32, f32, f32) {
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        if input_map.is_pressed(ACTION_FORWARD) {
            forward -= 1.0;
        }
        if input_map.is_pressed(ACTION_BACKWARD) {
            forward += 1.0;
        }
        if input_map.is_pressed(ACTION_LEFT) {
            right -= 1.0;
        }
        if input_map.is_pressed(ACTION_RIGHT) {
            right += 1.0;
        }
        if input_map.is_pressed(ACTION_UP) {
            up += 1.0;
        }
        if input_map.is_pressed(ACTION_DOWN) {
            up -= 1.0;
        }
        (forward, right, up)
    }

    pub(super) fn update(
        &self,
        transform: &mut khora_sdk::prelude::ecs::Transform,
        delta_time: f32,
        input_map: &InputMap,
    ) {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        let (move_forward, move_right, move_up) = Self::movement_axes(input_map);

        let velocity = self.speed * delta_time;
        transform.translation = transform.translation
            + forward * (-move_forward) * velocity
            + right * move_right * velocity
            + Vec3::Y * move_up * velocity;

        let yaw_quat = Quaternion::from_axis_angle(Vec3::Y, self.yaw);
        let pitch_quat = Quaternion::from_axis_angle(Vec3::X, self.pitch);
        transform.rotation = yaw_quat * pitch_quat;
    }
}
