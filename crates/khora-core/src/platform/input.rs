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

//! Abstract input event types used by the engine core.
//!
//! These types are backend-agnostic. Concrete windowing backends (winit,
//! SDL, etc.) translate their native events into these abstract events via
//! adapter functions in `khora-infra`.

/// An engine-internal representation of a user input event.
///
/// This enum is backend-agnostic and represents the high-level input actions
/// that the engine's systems can respond to.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A keyboard key was pressed (initial press, no auto-repeat).
    KeyPressed {
        /// The physical key code, type-safe.
        key_code: KeyCode,
    },
    /// A keyboard key was released.
    KeyReleased {
        /// The physical key code, type-safe.
        key_code: KeyCode,
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
        /// The new x-coordinate of the cursor (window pixels).
        x: f32,
        /// The new y-coordinate of the cursor (window pixels).
        y: f32,
    },
    /// The mouse wheel was scrolled.
    MouseWheelScrolled {
        /// Horizontal scroll delta.
        delta_x: f32,
        /// Vertical scroll delta.
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

/// Type-safe physical key codes — a 1-to-1 mirror of `winit::keyboard::KeyCode`
/// but living in `khora-core` so app code never has to depend on winit.
///
/// Variant names follow the W3C UI Events `code` attribute convention
/// (`KeyA`, `Digit1`, `ShiftLeft`, …) — same as winit. The
/// [`Unidentified`](Self::Unidentified) catch-all is used by the platform
/// adapter for keys that don't have a recognised code.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    Backquote,
    Backslash,
    BracketLeft,
    BracketRight,
    Comma,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Equal,
    IntlBackslash,
    IntlRo,
    IntlYen,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Minus,
    Period,
    Quote,
    Semicolon,
    Slash,
    AltLeft,
    AltRight,
    Backspace,
    CapsLock,
    ContextMenu,
    ControlLeft,
    ControlRight,
    Enter,
    SuperLeft,
    SuperRight,
    ShiftLeft,
    ShiftRight,
    Space,
    Tab,
    Convert,
    KanaMode,
    Lang1,
    Lang2,
    Lang3,
    Lang4,
    Lang5,
    NonConvert,
    Delete,
    End,
    Help,
    Home,
    Insert,
    PageDown,
    PageUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    NumLock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadBackspace,
    NumpadClear,
    NumpadClearEntry,
    NumpadComma,
    NumpadDecimal,
    NumpadDivide,
    NumpadEnter,
    NumpadEqual,
    NumpadHash,
    NumpadMemoryAdd,
    NumpadMemoryClear,
    NumpadMemoryRecall,
    NumpadMemoryStore,
    NumpadMemorySubtract,
    NumpadMultiply,
    NumpadParenLeft,
    NumpadParenRight,
    NumpadStar,
    NumpadSubtract,
    Escape,
    Fn,
    FnLock,
    PrintScreen,
    ScrollLock,
    Pause,
    BrowserBack,
    BrowserFavorites,
    BrowserForward,
    BrowserHome,
    BrowserRefresh,
    BrowserSearch,
    BrowserStop,
    Eject,
    LaunchApp1,
    LaunchApp2,
    LaunchMail,
    MediaPlayPause,
    MediaSelect,
    MediaStop,
    MediaTrackNext,
    MediaTrackPrevious,
    Power,
    Sleep,
    AudioVolumeDown,
    AudioVolumeMute,
    AudioVolumeUp,
    WakeUp,
    Meta,
    Hyper,
    Turbo,
    Abort,
    Resume,
    Suspend,
    Again,
    Copy,
    Cut,
    Find,
    Open,
    Paste,
    Props,
    Select,
    Undo,
    Hiragana,
    Katakana,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
    /// Catch-all for keys whose physical code the adapter could not map to
    /// any of the variants above. The platform adapter logs a one-shot
    /// warning the first time it produces this value so the gap is visible.
    Unidentified,
}
