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
//! These types are backend-agnostic. Concrete windowing backends (winit, SDL, etc.)
//! translate their native events into these abstract events via adapter functions.

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
