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

//! Core interfaces for the UI system.
//!
//! `theme` and `fonts` are app-agnostic structures shared by every Khora
//! UI surface (editor, hub, future tools). The `editor` submodule layers
//! the editor-specific framework (panels, dock, viewport handle, paint
//! `UiBuilder`) on top.

pub mod app;
pub mod editor;
pub mod editor_overlay;
pub mod fonts;
pub mod geometry;
pub mod layout;
pub mod theme;
pub mod types;

pub use app::{App, AppContext, AppLifecycle};
pub use editor::{
    EditorCamera, EditorPanel, EditorShell, PanelLocation, UiBuilder, ViewportTextureHandle,
};
pub use editor_overlay::{EditorOverlay, OverlayError, OverlayScreenDescriptor};
pub use fonts::{FontHandle, FontPack, NamedFont};
pub use geometry::{Align, Align2, CornerRadius, Margin, Stroke};
pub use layout::{LayoutSystem, UiLayoutView};
pub use theme::UiTheme;
