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

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

/// A new trait that combines the windowing handle traits required by graphics backends.
/// This is used to satisfy Rust's "trait object" rules.
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle {}

// We can automatically implement this new trait for any type that
// already implements the required subtraits. This is a powerful blanket implementation.
impl<T: HasWindowHandle + HasDisplayHandle> WindowHandle for T {}

// Now, our type alias uses our new, single, "normal" trait.
// `Send` and `Sync` can follow because they are "auto traits".
pub type KhoraWindowHandle = Arc<dyn WindowHandle + Send + Sync>;

/// A trait that abstracts the behavior of a window.
///
/// Any windowing backend (Winit, SDL2, Glfw, etc.) can implement this trait
/// to be compatible with the Khora engine.
pub trait KhoraWindow: HasWindowHandle + HasDisplayHandle + Send + Sync {
    /// Returns the physical dimensions (width, height) of the window's inner area.
    fn inner_size(&self) -> (u32, u32);

    /// Returns the scale factor of the window.
    fn scale_factor(&self) -> f64;

    /// Requests that the window be redrawn.
    fn request_redraw(&self);

    /// Clones an Arc'd, thread-safe handle to the window.
    /// This is necessary for the renderer to create a surface.
    fn clone_handle_arc(&self) -> KhoraWindowHandle;

    /// Returns the unique identifier for the window.
    fn id(&self) -> u64;

    // TODO: add more window management methods as needed
}
