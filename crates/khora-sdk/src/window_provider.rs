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

/// A platform window the engine renders into, hiding the windowing library from it.
pub trait WindowProvider: 'static {
    /// Creates the window on the native event loop `native_loop`.
    ///
    /// `native_loop` is opaque; the implementation downcasts it to its library's
    /// loop type, `winit::event_loop::ActiveEventLoop` for winit.
    fn create(native_loop: &dyn std::any::Any, config: &WindowConfig) -> Self
    where
        Self: Sized;

    /// Asks the platform to redraw the window.
    fn request_redraw(&self);

    /// Returns the current inner size of the window in physical pixels.
    fn inner_size(&self) -> (u32, u32);

    /// Returns the current DPI scaling factor for the window.
    fn scale_factor(&self) -> f64;

    /// Returns the window as the engine's `dyn KhoraWindow`.
    fn as_khora_window(&self) -> &dyn KhoraWindow;

    /// Translates a raw window event into an [`InputEvent`], or `None` when it is not input.
    fn translate_event(&self, raw_event: &dyn std::any::Any) -> Option<InputEvent>;

    /// Returns a long-lived handle to the native window, as an opaque `Arc<dyn Any>`.
    ///
    /// The winit runner downcasts it and puts the `Arc<winit::window::Window>`
    /// in the runtime's resources, so no winit type enters the SDK's API.
    fn clone_raw_window_arc(&self) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}
