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

//! The `WindowHandle` marker trait and the `KhoraWindowHandle` trait object
//! a renderer holds a window by.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::fmt::Debug;
use std::sync::Arc;

/// A marker trait that combines windowing handle requirements for use in trait objects.
///
/// Rust's trait object safety rules require that a trait's supertraits must also be
/// object-safe. `HasWindowHandle` and `HasDisplayHandle` are, but creating a direct
/// `dyn HasWindowHandle + HasDisplayHandle` is complex. This trait serves as a simple,
/// unified supertrait to make creating the `KhoraWindowHandle` type alias possible.
///
/// `Debug` is required so the handle can be stored inside `wgpu::InstanceDescriptor`
/// (its `display` field requires `WgpuHasDisplayHandle: Debug`).
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle + Debug {}

// A blanket implementation automatically implements `WindowHandle` for any type
// that already satisfies its requirements.
impl<T: HasWindowHandle + HasDisplayHandle + Debug> WindowHandle for T {}

/// A thread-safe, reference-counted trait object representing a window.
///
/// This type alias is used to pass a handle to a window across thread boundaries,
/// for example, from the main application thread to a rendering thread.
pub type KhoraWindowHandle = Arc<dyn WindowHandle + Send + Sync>;
