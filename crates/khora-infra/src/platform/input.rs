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

//! Provides translation from a concrete windowing backend (`winit`) to the engine's abstract input events.
//!
//! This module acts as an adapter layer, decoupling the rest of the engine from the
//! specific input event format of the `winit` crate.

use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// An engine-internal representation of a user input event.
///
/// This enum is backend-agnostic and represents the high-level input actions
/// that the engine's systems can respond to.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A keyboard key was pressed.
    KeyPressed {
        /// A string representation of the physical key code.
        key_code: String,
    },
    /// A keyboard key was released.
    KeyReleased {
        /// A string representation of the physical key code.
        key_code: String,
    },
    /// A mouse button was pressed.
    MouseButtonPressed {
        /// The mouse button that was pressed.
        button: MouseButton,
    },
    /// A mouse button was released.
    MouseButtonReleased {
        /// The mouse button that was released.
        button: MouseButton,
    },
    /// The mouse cursor moved.
    MouseMoved {
        /// The new x-coordinate of the cursor.
        x: f32,
        /// The new y-coordinate of the cursor.
        y: f32,
    },
    /// The mouse wheel was scrolled.
    MouseWheelScrolled {
        /// The horizontal scroll delta.
        delta_x: f32,
        /// The vertical scroll delta.
        delta_y: f32,
    },
}

/// An engine-internal representation of a mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The left mouse button.
    Left,
    /// The right mouse button.
    Right,
    /// The middle mouse button.
    Middle,
    /// The back mouse button (typically on the side).
    Back,
    /// The forward mouse button (typically on the side).
    Forward,
    /// Another mouse button, identified by a numeric code.
    Other(u16),
}

/// Translates a `winit::event::WindowEvent` into Khora's `InputEvent` format.
///
/// This function acts as an adapter, filtering and converting raw windowing events
/// into a format that the engine's core systems can understand and process. It ignores
/// events that are not direct user input actions (e.g., window resizing, focus changes).
///
/// # Arguments
///
/// * `event`: A reference to a `WindowEvent` from the `winit` library.
///
/// # Returns
///
/// Returns `Some(InputEvent)` if the event is a recognized input action, or `None` otherwise.
pub fn translate_winit_input(event: &WindowEvent) -> Option<InputEvent> {
    match event {
        WindowEvent::KeyboardInput {
            event: key_event, ..
        } => {
            if let PhysicalKey::Code(keycode) = key_event.physical_key {
                let key_code_str = map_keycode_to_string(keycode);
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => Some(InputEvent::KeyPressed {
                        key_code: key_code_str,
                    }),
                    ElementState::Released => Some(InputEvent::KeyReleased {
                        key_code: key_code_str,
                    }),
                    _ => None,
                }
            } else {
                None
            }
        }
        WindowEvent::CursorMoved { position, .. } => Some(InputEvent::MouseMoved {
            x: position.x as f32,
            y: position.y as f32,
        }),
        WindowEvent::MouseInput { state, button, .. } => {
            let khora_button = map_mouse_button(*button);
            match state {
                ElementState::Pressed => Some(InputEvent::MouseButtonPressed {
                    button: khora_button,
                }),
                ElementState::Released => Some(InputEvent::MouseButtonReleased {
                    button: khora_button,
                }),
            }
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let (dx, dy): (f32, f32) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (*x, *y),
                MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
            };
            if dx != 0.0 || dy != 0.0 {
                Some(InputEvent::MouseWheelScrolled {
                    delta_x: dx,
                    delta_y: dy,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

// --- Private Helper Functions ---

/// (Internal) Maps a `winit::keyboard::KeyCode` to a string representation.
fn map_keycode_to_string(keycode: KeyCode) -> String {
    format!("{keycode:?}")
}

/// (Internal) Maps a `winit::event::MouseButton` to the engine's `MouseButton` enum.
fn map_mouse_button(button: WinitMouseButton) -> MouseButton {
    match button {
        WinitMouseButton::Left => MouseButton::Left,
        WinitMouseButton::Right => MouseButton::Right,
        WinitMouseButton::Middle => MouseButton::Middle,
        WinitMouseButton::Back => MouseButton::Back,
        WinitMouseButton::Forward => MouseButton::Forward,
        WinitMouseButton::Other(id) => MouseButton::Other(id),
    }
}

// --- Unit Tests for Input Translation ---
#[cfg(test)]
mod tests {
    use super::*;
    use winit::{dpi::PhysicalPosition, event::WindowEvent, keyboard::KeyCode};

    /// Test cases for translating keycodes to strings
    #[test]
    fn test_map_keycode_simple() {
        assert_eq!(map_keycode_to_string(KeyCode::KeyA), "KeyA");
        assert_eq!(map_keycode_to_string(KeyCode::Digit1), "Digit1");
        assert_eq!(map_keycode_to_string(KeyCode::Space), "Space");
    }

    /// Test cases for translating mouse buttons to the engine's internal representation
    #[test]
    fn test_map_mouse_button_standard() {
        assert_eq!(map_mouse_button(WinitMouseButton::Left), MouseButton::Left);
        assert_eq!(
            map_mouse_button(WinitMouseButton::Right),
            MouseButton::Right
        );
        assert_eq!(
            map_mouse_button(WinitMouseButton::Middle),
            MouseButton::Middle
        );
        assert_eq!(map_mouse_button(WinitMouseButton::Back), MouseButton::Back);
        assert_eq!(
            map_mouse_button(WinitMouseButton::Forward),
            MouseButton::Forward
        );
    }

    /// Test cases for translating other mouse buttons to the engine's internal representation
    #[test]
    fn test_map_mouse_button_other() {
        assert_eq!(
            map_mouse_button(WinitMouseButton::Other(8)),
            MouseButton::Other(8)
        );
        assert_eq!(
            map_mouse_button(WinitMouseButton::Other(15)),
            MouseButton::Other(15)
        );
    }

    /// Test cases for translating winit mouse press to engine's internal representation
    #[test]
    fn test_translate_mouse_button_pressed() {
        let winit_event = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
        };
        let expected = Some(InputEvent::MouseButtonPressed {
            button: MouseButton::Left,
        });
        assert_eq!(translate_winit_input(&winit_event), expected);
    }

    /// Test cases for translating winit mouse release to engine's internal representation
    #[test]
    fn test_translate_mouse_button_released() {
        let winit_event = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Released,
            button: WinitMouseButton::Right,
        };
        let expected = Some(InputEvent::MouseButtonReleased {
            button: MouseButton::Right,
        });
        assert_eq!(translate_winit_input(&winit_event), expected);
    }

    /// Test cases for translating winit cursor movement to engine's internal representation
    #[test]
    fn test_translate_cursor_moved() {
        let winit_event = WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: PhysicalPosition::new(100.5, 200.75),
        };
        let expected = Some(InputEvent::MouseMoved {
            x: 100.5,
            y: 200.75,
        });
        assert_eq!(translate_winit_input(&winit_event), expected);
    }

    /// Test cases for translating winit mouse wheel scroll to engine's internal representation
    #[test]
    fn test_translate_mouse_wheel_line() {
        let winit_event = WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(-1.0, 2.0),
            phase: winit::event::TouchPhase::Moved,
        };
        let expected = Some(InputEvent::MouseWheelScrolled {
            delta_x: -1.0,
            delta_y: 2.0,
        });
        assert_eq!(translate_winit_input(&winit_event), expected);
    }

    /// Test cases for translating winit mouse wheel scroll in pixels to engine's internal representation
    #[test]
    fn test_translate_mouse_wheel_pixel() {
        let winit_event = WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(5.5, -10.0)),
            phase: winit::event::TouchPhase::Moved,
        };
        let expected = Some(InputEvent::MouseWheelScrolled {
            delta_x: 5.5,
            delta_y: -10.0,
        });
        assert_eq!(translate_winit_input(&winit_event), expected);
    }

    /// Test cases for translating winit specific window events to engine's internal representation
    #[test]
    fn test_translate_non_input_returns_none() {
        let winit_event_resize = WindowEvent::Resized(winit::dpi::PhysicalSize::new(100, 100));
        let winit_event_focus = WindowEvent::Focused(true);
        let winit_event_close = WindowEvent::CloseRequested;
        assert_eq!(translate_winit_input(&winit_event_resize), None);
        assert_eq!(translate_winit_input(&winit_event_focus), None);
        assert_eq!(translate_winit_input(&winit_event_close), None);
    }
}
