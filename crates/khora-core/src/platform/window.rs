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

//! Defines the `KhoraWindow` trait and related types for windowing abstraction.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

/// A marker trait that combines windowing handle requirements for use in trait objects.
///
/// Rust's trait object safety rules require that a trait's supertraits must also be
/// object-safe. `HasWindowHandle` and `HasDisplayHandle` are, but creating a direct
/// `dyn HasWindowHandle + HasDisplayHandle` is complex. This trait serves as a simple,
/// unified supertrait to make creating the `KhoraWindowHandle` type alias possible.
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle {}

// A blanket implementation automatically implements `WindowHandle` for any type
// that already satisfies its requirements.
impl<T: HasWindowHandle + HasDisplayHandle> WindowHandle for T {}

/// A thread-safe, reference-counted trait object representing a window.
///
/// This type alias is used to pass a handle to a window across thread boundaries,
/// for example, from the main application thread to a rendering thread.
pub type KhoraWindowHandle = Arc<dyn WindowHandle + Send + Sync>;

/// A trait that abstracts the behavior of an application window.
///
/// This is the primary contract for windowing integration in Khora. Any windowing
/// backend (like the Winit implementation in `khora-infra`) must implement this trait
/// to be usable by the engine's rendering and input systems.
pub trait KhoraWindow: HasWindowHandle + HasDisplayHandle + Send + Sync {
    /// Returns the physical dimensions (width, height) in pixels of the window's inner client area.
    fn inner_size(&self) -> (u32, u32);

    /// Returns the display's scale factor, used for HiDPI rendering.
    fn scale_factor(&self) -> f64;

    /// Requests that the operating system schedule a redraw for the window.
    fn request_redraw(&self);

    /// Clones a thread-safe, reference-counted handle to the window.
    ///
    /// This is the primary mechanism for the renderer to obtain a handle it can use
    /// to create a render surface, without needing to know the concrete window type.
    fn clone_handle_arc(&self) -> KhoraWindowHandle;

    /// Returns a unique identifier for the window.
    fn id(&self) -> u64;
}
