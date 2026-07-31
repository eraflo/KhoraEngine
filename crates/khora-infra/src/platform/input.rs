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
//! The abstract types (`InputEvent`, `KeyCode`, `MouseButton`) live in
//! `khora_core::platform::input`. This module only provides the
//! `winit`-specific translation function plus the 1-to-1 winit→Khora
//! `KeyCode` table.

use std::sync::OnceLock;

use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode as WinitKeyCode, PhysicalKey};

use khora_core::platform::{InputEvent, KeyCode, MouseButton};

/// Translates a `winit::event::WindowEvent` into Khora's `InputEvent` format.
///
/// Returns `Some(InputEvent)` for recognized input actions, `None` for window
/// lifecycle events (resize, focus, etc.) the engine doesn't care about.
pub fn translate_winit_input(event: &WindowEvent) -> Option<InputEvent> {
    match event {
        WindowEvent::KeyboardInput {
            event: key_event, ..
        } => {
            if let PhysicalKey::Code(keycode) = key_event.physical_key {
                let key_code = map_keycode(keycode);
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => {
                        Some(InputEvent::KeyPressed { key_code })
                    }
                    ElementState::Released => Some(InputEvent::KeyReleased { key_code }),
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

/// 1-to-1 mapping from `winit::keyboard::KeyCode` to Khora's typed
/// [`KeyCode`]. Names line up exactly with the W3C UI Events `code`
/// attribute on both sides. Anything we miss (winit gains a new variant,
/// platform reports something exotic) lands on
/// [`KeyCode::Unidentified`] with a one-shot warning so the gap is
/// observable.
pub(crate) fn map_keycode(keycode: WinitKeyCode) -> KeyCode {
    match keycode {
        WinitKeyCode::Backquote => KeyCode::Backquote,
        WinitKeyCode::Backslash => KeyCode::Backslash,
        WinitKeyCode::BracketLeft => KeyCode::BracketLeft,
        WinitKeyCode::BracketRight => KeyCode::BracketRight,
        WinitKeyCode::Comma => KeyCode::Comma,
        WinitKeyCode::Digit0 => KeyCode::Digit0,
        WinitKeyCode::Digit1 => KeyCode::Digit1,
        WinitKeyCode::Digit2 => KeyCode::Digit2,
        WinitKeyCode::Digit3 => KeyCode::Digit3,
        WinitKeyCode::Digit4 => KeyCode::Digit4,
        WinitKeyCode::Digit5 => KeyCode::Digit5,
        WinitKeyCode::Digit6 => KeyCode::Digit6,
        WinitKeyCode::Digit7 => KeyCode::Digit7,
        WinitKeyCode::Digit8 => KeyCode::Digit8,
        WinitKeyCode::Digit9 => KeyCode::Digit9,
        WinitKeyCode::Equal => KeyCode::Equal,
        WinitKeyCode::IntlBackslash => KeyCode::IntlBackslash,
        WinitKeyCode::IntlRo => KeyCode::IntlRo,
        WinitKeyCode::IntlYen => KeyCode::IntlYen,
        WinitKeyCode::KeyA => KeyCode::KeyA,
        WinitKeyCode::KeyB => KeyCode::KeyB,
        WinitKeyCode::KeyC => KeyCode::KeyC,
        WinitKeyCode::KeyD => KeyCode::KeyD,
        WinitKeyCode::KeyE => KeyCode::KeyE,
        WinitKeyCode::KeyF => KeyCode::KeyF,
        WinitKeyCode::KeyG => KeyCode::KeyG,
        WinitKeyCode::KeyH => KeyCode::KeyH,
        WinitKeyCode::KeyI => KeyCode::KeyI,
        WinitKeyCode::KeyJ => KeyCode::KeyJ,
        WinitKeyCode::KeyK => KeyCode::KeyK,
        WinitKeyCode::KeyL => KeyCode::KeyL,
        WinitKeyCode::KeyM => KeyCode::KeyM,
        WinitKeyCode::KeyN => KeyCode::KeyN,
        WinitKeyCode::KeyO => KeyCode::KeyO,
        WinitKeyCode::KeyP => KeyCode::KeyP,
        WinitKeyCode::KeyQ => KeyCode::KeyQ,
        WinitKeyCode::KeyR => KeyCode::KeyR,
        WinitKeyCode::KeyS => KeyCode::KeyS,
        WinitKeyCode::KeyT => KeyCode::KeyT,
        WinitKeyCode::KeyU => KeyCode::KeyU,
        WinitKeyCode::KeyV => KeyCode::KeyV,
        WinitKeyCode::KeyW => KeyCode::KeyW,
        WinitKeyCode::KeyX => KeyCode::KeyX,
        WinitKeyCode::KeyY => KeyCode::KeyY,
        WinitKeyCode::KeyZ => KeyCode::KeyZ,
        WinitKeyCode::Minus => KeyCode::Minus,
        WinitKeyCode::Period => KeyCode::Period,
        WinitKeyCode::Quote => KeyCode::Quote,
        WinitKeyCode::Semicolon => KeyCode::Semicolon,
        WinitKeyCode::Slash => KeyCode::Slash,
        WinitKeyCode::AltLeft => KeyCode::AltLeft,
        WinitKeyCode::AltRight => KeyCode::AltRight,
        WinitKeyCode::Backspace => KeyCode::Backspace,
        WinitKeyCode::CapsLock => KeyCode::CapsLock,
        WinitKeyCode::ContextMenu => KeyCode::ContextMenu,
        WinitKeyCode::ControlLeft => KeyCode::ControlLeft,
        WinitKeyCode::ControlRight => KeyCode::ControlRight,
        WinitKeyCode::Enter => KeyCode::Enter,
        WinitKeyCode::SuperLeft => KeyCode::SuperLeft,
        WinitKeyCode::SuperRight => KeyCode::SuperRight,
        WinitKeyCode::ShiftLeft => KeyCode::ShiftLeft,
        WinitKeyCode::ShiftRight => KeyCode::ShiftRight,
        WinitKeyCode::Space => KeyCode::Space,
        WinitKeyCode::Tab => KeyCode::Tab,
        WinitKeyCode::Convert => KeyCode::Convert,
        WinitKeyCode::KanaMode => KeyCode::KanaMode,
        WinitKeyCode::Lang1 => KeyCode::Lang1,
        WinitKeyCode::Lang2 => KeyCode::Lang2,
        WinitKeyCode::Lang3 => KeyCode::Lang3,
        WinitKeyCode::Lang4 => KeyCode::Lang4,
        WinitKeyCode::Lang5 => KeyCode::Lang5,
        WinitKeyCode::NonConvert => KeyCode::NonConvert,
        WinitKeyCode::Delete => KeyCode::Delete,
        WinitKeyCode::End => KeyCode::End,
        WinitKeyCode::Help => KeyCode::Help,
        WinitKeyCode::Home => KeyCode::Home,
        WinitKeyCode::Insert => KeyCode::Insert,
        WinitKeyCode::PageDown => KeyCode::PageDown,
        WinitKeyCode::PageUp => KeyCode::PageUp,
        WinitKeyCode::ArrowDown => KeyCode::ArrowDown,
        WinitKeyCode::ArrowLeft => KeyCode::ArrowLeft,
        WinitKeyCode::ArrowRight => KeyCode::ArrowRight,
        WinitKeyCode::ArrowUp => KeyCode::ArrowUp,
        WinitKeyCode::NumLock => KeyCode::NumLock,
        WinitKeyCode::Numpad0 => KeyCode::Numpad0,
        WinitKeyCode::Numpad1 => KeyCode::Numpad1,
        WinitKeyCode::Numpad2 => KeyCode::Numpad2,
        WinitKeyCode::Numpad3 => KeyCode::Numpad3,
        WinitKeyCode::Numpad4 => KeyCode::Numpad4,
        WinitKeyCode::Numpad5 => KeyCode::Numpad5,
        WinitKeyCode::Numpad6 => KeyCode::Numpad6,
        WinitKeyCode::Numpad7 => KeyCode::Numpad7,
        WinitKeyCode::Numpad8 => KeyCode::Numpad8,
        WinitKeyCode::Numpad9 => KeyCode::Numpad9,
        WinitKeyCode::NumpadAdd => KeyCode::NumpadAdd,
        WinitKeyCode::NumpadBackspace => KeyCode::NumpadBackspace,
        WinitKeyCode::NumpadClear => KeyCode::NumpadClear,
        WinitKeyCode::NumpadClearEntry => KeyCode::NumpadClearEntry,
        WinitKeyCode::NumpadComma => KeyCode::NumpadComma,
        WinitKeyCode::NumpadDecimal => KeyCode::NumpadDecimal,
        WinitKeyCode::NumpadDivide => KeyCode::NumpadDivide,
        WinitKeyCode::NumpadEnter => KeyCode::NumpadEnter,
        WinitKeyCode::NumpadEqual => KeyCode::NumpadEqual,
        WinitKeyCode::NumpadHash => KeyCode::NumpadHash,
        WinitKeyCode::NumpadMemoryAdd => KeyCode::NumpadMemoryAdd,
        WinitKeyCode::NumpadMemoryClear => KeyCode::NumpadMemoryClear,
        WinitKeyCode::NumpadMemoryRecall => KeyCode::NumpadMemoryRecall,
        WinitKeyCode::NumpadMemoryStore => KeyCode::NumpadMemoryStore,
        WinitKeyCode::NumpadMemorySubtract => KeyCode::NumpadMemorySubtract,
        WinitKeyCode::NumpadMultiply => KeyCode::NumpadMultiply,
        WinitKeyCode::NumpadParenLeft => KeyCode::NumpadParenLeft,
        WinitKeyCode::NumpadParenRight => KeyCode::NumpadParenRight,
        WinitKeyCode::NumpadStar => KeyCode::NumpadStar,
        WinitKeyCode::NumpadSubtract => KeyCode::NumpadSubtract,
        WinitKeyCode::Escape => KeyCode::Escape,
        WinitKeyCode::Fn => KeyCode::Fn,
        WinitKeyCode::FnLock => KeyCode::FnLock,
        WinitKeyCode::PrintScreen => KeyCode::PrintScreen,
        WinitKeyCode::ScrollLock => KeyCode::ScrollLock,
        WinitKeyCode::Pause => KeyCode::Pause,
        WinitKeyCode::BrowserBack => KeyCode::BrowserBack,
        WinitKeyCode::BrowserFavorites => KeyCode::BrowserFavorites,
        WinitKeyCode::BrowserForward => KeyCode::BrowserForward,
        WinitKeyCode::BrowserHome => KeyCode::BrowserHome,
        WinitKeyCode::BrowserRefresh => KeyCode::BrowserRefresh,
        WinitKeyCode::BrowserSearch => KeyCode::BrowserSearch,
        WinitKeyCode::BrowserStop => KeyCode::BrowserStop,
        WinitKeyCode::Eject => KeyCode::Eject,
        WinitKeyCode::LaunchApp1 => KeyCode::LaunchApp1,
        WinitKeyCode::LaunchApp2 => KeyCode::LaunchApp2,
        WinitKeyCode::LaunchMail => KeyCode::LaunchMail,
        WinitKeyCode::MediaPlayPause => KeyCode::MediaPlayPause,
        WinitKeyCode::MediaSelect => KeyCode::MediaSelect,
        WinitKeyCode::MediaStop => KeyCode::MediaStop,
        WinitKeyCode::MediaTrackNext => KeyCode::MediaTrackNext,
        WinitKeyCode::MediaTrackPrevious => KeyCode::MediaTrackPrevious,
        WinitKeyCode::Power => KeyCode::Power,
        WinitKeyCode::Sleep => KeyCode::Sleep,
        WinitKeyCode::AudioVolumeDown => KeyCode::AudioVolumeDown,
        WinitKeyCode::AudioVolumeMute => KeyCode::AudioVolumeMute,
        WinitKeyCode::AudioVolumeUp => KeyCode::AudioVolumeUp,
        WinitKeyCode::WakeUp => KeyCode::WakeUp,
        WinitKeyCode::Meta => KeyCode::Meta,
        WinitKeyCode::Hyper => KeyCode::Hyper,
        WinitKeyCode::Turbo => KeyCode::Turbo,
        WinitKeyCode::Abort => KeyCode::Abort,
        WinitKeyCode::Resume => KeyCode::Resume,
        WinitKeyCode::Suspend => KeyCode::Suspend,
        WinitKeyCode::Again => KeyCode::Again,
        WinitKeyCode::Copy => KeyCode::Copy,
        WinitKeyCode::Cut => KeyCode::Cut,
        WinitKeyCode::Find => KeyCode::Find,
        WinitKeyCode::Open => KeyCode::Open,
        WinitKeyCode::Paste => KeyCode::Paste,
        WinitKeyCode::Props => KeyCode::Props,
        WinitKeyCode::Select => KeyCode::Select,
        WinitKeyCode::Undo => KeyCode::Undo,
        WinitKeyCode::Hiragana => KeyCode::Hiragana,
        WinitKeyCode::Katakana => KeyCode::Katakana,
        WinitKeyCode::F1 => KeyCode::F1,
        WinitKeyCode::F2 => KeyCode::F2,
        WinitKeyCode::F3 => KeyCode::F3,
        WinitKeyCode::F4 => KeyCode::F4,
        WinitKeyCode::F5 => KeyCode::F5,
        WinitKeyCode::F6 => KeyCode::F6,
        WinitKeyCode::F7 => KeyCode::F7,
        WinitKeyCode::F8 => KeyCode::F8,
        WinitKeyCode::F9 => KeyCode::F9,
        WinitKeyCode::F10 => KeyCode::F10,
        WinitKeyCode::F11 => KeyCode::F11,
        WinitKeyCode::F12 => KeyCode::F12,
        WinitKeyCode::F13 => KeyCode::F13,
        WinitKeyCode::F14 => KeyCode::F14,
        WinitKeyCode::F15 => KeyCode::F15,
        WinitKeyCode::F16 => KeyCode::F16,
        WinitKeyCode::F17 => KeyCode::F17,
        WinitKeyCode::F18 => KeyCode::F18,
        WinitKeyCode::F19 => KeyCode::F19,
        WinitKeyCode::F20 => KeyCode::F20,
        WinitKeyCode::F21 => KeyCode::F21,
        WinitKeyCode::F22 => KeyCode::F22,
        WinitKeyCode::F23 => KeyCode::F23,
        WinitKeyCode::F24 => KeyCode::F24,
        WinitKeyCode::F25 => KeyCode::F25,
        WinitKeyCode::F26 => KeyCode::F26,
        WinitKeyCode::F27 => KeyCode::F27,
        WinitKeyCode::F28 => KeyCode::F28,
        WinitKeyCode::F29 => KeyCode::F29,
        WinitKeyCode::F30 => KeyCode::F30,
        WinitKeyCode::F31 => KeyCode::F31,
        WinitKeyCode::F32 => KeyCode::F32,
        WinitKeyCode::F33 => KeyCode::F33,
        WinitKeyCode::F34 => KeyCode::F34,
        WinitKeyCode::F35 => KeyCode::F35,
        // winit::KeyCode is `#[non_exhaustive]` — newer winit releases may
        // add variants we don't yet know about. Rather than fall through
        // silently, emit a one-shot warning and surface the gap to the
        // engine via `KeyCode::Unidentified`.
        other => {
            static WARNED: OnceLock<()> = OnceLock::new();
            if WARNED.set(()).is_ok() {
                log::warn!(
                    "khora-infra: unmapped winit::KeyCode variant {:?} (using KeyCode::Unidentified). \
                     Consider extending map_keycode in crates/khora-infra/src/platform/input.rs.",
                    other,
                );
            }
            KeyCode::Unidentified
        }
    }
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
    use winit::{dpi::PhysicalPosition, event::WindowEvent};

    #[test]
    fn test_map_keycode_simple() {
        assert_eq!(map_keycode(WinitKeyCode::KeyA), KeyCode::KeyA);
        assert_eq!(map_keycode(WinitKeyCode::Digit1), KeyCode::Digit1);
        assert_eq!(map_keycode(WinitKeyCode::Space), KeyCode::Space);
        assert_eq!(map_keycode(WinitKeyCode::ShiftLeft), KeyCode::ShiftLeft);
        assert_eq!(map_keycode(WinitKeyCode::F12), KeyCode::F12);
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
    }

    #[test]
    fn test_translate_mouse_button_pressed() {
        let winit_event = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
        };
        assert_eq!(
            translate_winit_input(&winit_event),
            Some(InputEvent::MouseButtonPressed {
                button: MouseButton::Left,
            })
        );
    }

    #[test]
    fn test_translate_cursor_moved() {
        let winit_event = WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: PhysicalPosition::new(100.5, 200.75),
        };
        assert_eq!(
            translate_winit_input(&winit_event),
            Some(InputEvent::MouseMoved {
                x: 100.5,
                y: 200.75,
            })
        );
    }

    #[test]
    fn test_translate_mouse_wheel_line() {
        let winit_event = WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(-1.0, 2.0),
            phase: winit::event::TouchPhase::Moved,
        };
        assert_eq!(
            translate_winit_input(&winit_event),
            Some(InputEvent::MouseWheelScrolled {
                delta_x: -1.0,
                delta_y: 2.0,
            })
        );
    }

    #[test]
    fn test_translate_non_input_returns_none() {
        let winit_event_resize = WindowEvent::Resized(winit::dpi::PhysicalSize::new(100, 100));
        assert_eq!(translate_winit_input(&winit_event_resize), None);
    }
}
