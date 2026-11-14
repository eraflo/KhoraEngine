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

//! `WindowProvider`: abstracts the platform window backend.

use khora_core::platform::KhoraWindow;

use crate::InputEvent;
use crate::WindowConfig;

/// Provides a platform window for the engine to render into.
///
/// The engine doesn't know about winit, SDL, or any specific windowing library.
/// This trait abstracts window creation, event polling, and surface queries.
pub trait WindowProvider: 'static {
    /// Creates the window.
    ///
    /// The `native_loop` parameter is an opaque handle to the native event loop.
    /// The implementation must downcast it to the correct type.
    /// For winit: `native_loop.downcast_ref::<winit::event_loop::ActiveEventLoop>()`.
    fn create(native_loop: &dyn std::any::Any, config: &WindowConfig) -> Self
    where
        Self: Sized;

    /// Polls for window events and returns them as an iterator of raw event types.
    fn request_redraw(&self);

    /// Returns the current inner size of the window in physical pixels.
    fn inner_size(&self) -> (u32, u32);

    /// Returns the current DPI scaling factor for the window.
    fn scale_factor(&self) -> f64;

    /// Returns a reference to the window as a `dyn KhoraWindow` for use by the engine and agents.
    fn as_khora_window(&self) -> &dyn KhoraWindow;

    /// Translates a raw window event (e.g., winit::event::Event) into an engine-agnostic `InputEvent` that can be forwarded to game logic.
    fn translate_event(&self, raw_event: &dyn std::any::Any) -> Option<InputEvent>;

    /// Clones a long-lived handle to the raw native window as an opaque
    /// `Arc<dyn Any>`. Used by the winit runner to insert the underlying
    /// `Arc<winit::window::Window>` into the service registry so that
    /// editor hooks (e.g., overlay `begin_frame`) can retrieve it without
    /// the SDK leaking winit types into application code.
    fn clone_raw_window_arc(&self) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}
