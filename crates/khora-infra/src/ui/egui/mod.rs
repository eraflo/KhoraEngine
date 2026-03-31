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

//! Egui backend for the Khora Engine editor overlay.
//!
//! This module provides:
//! - [`EguiOverlay`] — the concrete [`EditorOverlay`](khora_core::ui::EditorOverlay)
//!   implementation backed by egui + a custom wgpu renderer.
//! - [`EguiWgpuRenderer`] — low-level wgpu rendering of egui primitives.
//! - [`EguiFrameRenderState`] — per-frame GPU state passed to the overlay.

pub mod overlay;
pub mod palette;
pub mod renderer;
pub mod shell;
pub mod theme;
pub mod ui_builder;

pub use overlay::{EguiFrameRenderState, EguiOverlay};
pub use renderer::EguiWgpuRenderer;
pub use shell::EguiEditorShell;
pub use ui_builder::EguiUiBuilder;
