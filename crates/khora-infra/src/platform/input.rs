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

//! Adapter layer: translates winit window events into Khora's abstract input events.
//!
//! The abstract types (`InputEvent`, `MouseButton`) live in `khora_core::platform::input`.
//! This module only provides the `winit`-specific translation function.

use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use khora_core::platform::{InputEvent, MouseButton};

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

fn map_keycode_to_string(keycode: KeyCode) -> String {
    format!("{keycode:?}")
}

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

// --- Unit Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use winit::{dpi::PhysicalPosition, event::WindowEvent, keyboard::KeyCode};

    #[test]
    fn test_map_keycode_simple() {
        assert_eq!(map_keycode_to_string(KeyCode::KeyA), "KeyA");
        assert_eq!(map_keycode_to_string(KeyCode::Digit1), "Digit1");
        assert_eq!(map_keycode_to_string(KeyCode::Space), "Space");
    }

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
