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

//! Abstract editor shell — owns the dock layout, menu bar, and toolbar.

use super::panel::{EditorPanel, PanelLocation};
use super::state::{EditorState, StatusBarData};
use super::theme::EditorTheme;

/// The top-level editor shell (menu bar + toolbar + docked panels).
///
/// Concrete implementations live in `khora-infra`. The engine calls
/// [`show_frame()`](Self::show_frame) once per frame between
/// `overlay.begin_frame()` and `overlay.end_frame_and_render()`.
pub trait EditorShell: Send + Sync {
    /// Registers a panel at the given dock location.
    fn register_panel(&mut self, location: PanelLocation, panel: Box<dyn EditorPanel>);

    /// Removes a panel by id. Returns `true` if it was found.
    fn remove_panel(&mut self, id: &str) -> bool;

    /// Applies a theme to the underlying UI backend.
    fn set_theme(&mut self, theme: EditorTheme);

    /// Updates the status bar data displayed at the bottom of the editor.
    fn set_status(&mut self, data: StatusBarData);

    /// Sets a shared `EditorState` reference so the shell can read/write
    /// gizmo mode, menu actions, etc.
    fn set_editor_state(&mut self, state: std::sync::Arc<std::sync::Mutex<EditorState>>);

    /// Renders the full editor frame (menu bar, toolbar, dock panels).
    ///
    /// Called by the engine each frame. Panels registered via
    /// [`register_panel`](Self::register_panel) will have their
    /// [`EditorPanel::ui`] method invoked.
    fn show_frame(&mut self);
}
