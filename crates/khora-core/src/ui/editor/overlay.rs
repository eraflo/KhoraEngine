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

//! Abstract trait for editor overlay rendering.
//!
//! The [`EditorOverlay`] trait defines the interface for rendering an immediate-mode
//! UI overlay on top of the engine's 3D scene. The concrete implementation lives in
//! `khora-infra` (e.g., `EguiOverlay` backed by egui + wgpu).
//!
//! # Architecture
//!
//! ```text
//! khora-core   → EditorOverlay trait (this file)
//! khora-infra  → EguiOverlay : impl EditorOverlay (egui + custom wgpu renderer)
//! khora-sdk    → EngineState integrates the overlay into the frame loop
//! khora-editor → Application that builds the editor UI via the shared context
//! ```

use std::any::Any;
use std::fmt;

/// Descriptor for the current screen state, passed to the overlay each frame.
#[derive(Debug, Clone, Copy)]
pub struct OverlayScreenDescriptor {
    /// Width of the render target in physical pixels.
    pub width_px: u32,
    /// Height of the render target in physical pixels.
    pub height_px: u32,
    /// HiDPI scale factor (physical pixels per logical point).
    pub scale_factor: f32,
}

/// Error type for overlay operations.
#[derive(Debug)]
pub struct OverlayError(pub String);

impl fmt::Display for OverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OverlayError: {}", self.0)
    }
}

impl std::error::Error for OverlayError {}

/// Abstract trait for an editor UI overlay rendered on top of the 3D scene.
///
/// The overlay manages its own UI context (e.g., `egui::Context`) and renderer.
/// It processes input events, builds UI each frame, and renders the result
/// as a final pass over the swapchain texture.
///
/// # Lifecycle per frame
///
/// 1. [`handle_window_event`](Self::handle_window_event) — called for each raw
///    window event (winit). Returns `true` if the overlay consumed the event.
/// 2. [`begin_frame`](Self::begin_frame) — starts a new UI frame.
/// 3. *(Application builds UI using the context returned by [`ui_context`](Self::ui_context))*
/// 4. [`end_frame_and_render`](Self::end_frame_and_render) — finalizes the UI,
///    tessellates, and renders onto the current render target.
pub trait EditorOverlay: Send + Sync {
    /// Process a raw window event for the overlay.
    ///
    /// The `event` parameter is a type-erased `winit::event::WindowEvent`.
    ///
    /// Returns `true` if the overlay consumed the event (e.g., the cursor is
    /// over an overlay panel). When `true`, the engine should **not** forward
    /// the event to the game's input system.
    fn handle_window_event(&mut self, window: &dyn Any, event: &dyn Any) -> bool;

    /// Starts a new overlay frame.
    ///
    /// `window` is a type-erased `winit::window::Window` reference, used by
    /// the input translation layer to read screen size and scale factor.
    ///
    /// Must be called exactly once per frame, before the application builds its UI.
    fn begin_frame(&mut self, window: &dyn Any, screen: OverlayScreenDescriptor);

    /// Returns the UI context as a type-erased reference.
    ///
    /// The concrete type is `egui::Context` for the egui backend.
    /// The editor (`khora-editor`) downcasts this to build its panels.
    fn ui_context(&self) -> &dyn Any;

    /// Ends the current frame and renders the overlay.
    ///
    /// `render_state` is a type-erased struct containing the GPU resources
    /// needed for rendering (device, queue, encoder, target view, etc.).
    /// The concrete type depends on the backend.
    fn end_frame_and_render(&mut self, render_state: &mut dyn Any) -> Result<(), OverlayError>;

    /// Returns `true` if the overlay wants exclusive pointer input this frame.
    ///
    /// When `true`, pointer events (clicks, drags) should not be forwarded to the game.
    fn wants_pointer_input(&self) -> bool;

    /// Returns `true` if the overlay wants exclusive keyboard input this frame.
    ///
    /// When `true`, keyboard events should not be forwarded to the game.
    fn wants_keyboard_input(&self) -> bool;

    /// Downcasting support.
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcasting support.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
