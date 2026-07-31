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

//! Abstract editor shell — generic host for dock-based panel layouts.
//!
//! The shell owns no application logic and no branding. It only knows how to:
//! - apply a theme to the underlying UI backend,
//! - lay out panels in a small set of fixed slots ([`PanelLocation`]),
//! - render those panels each frame via the [`UiBuilder`] abstraction.
//!
//! Concrete implementations (e.g. egui) live in `khora-infra`. Editor-specific
//! chrome (menu bars, toolbars, status bars, brand logos) is implemented as
//! ordinary panels in the application crate (`khora-editor`) — never in the
//! shell or the backend.

use super::panel::{EditorPanel, PanelLocation};
use super::state::{EditorState, StatusBarData};
use crate::ui::{FontPack, UiTheme};

/// The top-level editor shell — a generic host for docked panels.
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
    fn set_theme(&mut self, theme: UiTheme);

    /// Installs a custom font pack. If [`FontPack::is_empty`] is true, the
    /// backend keeps its built-in defaults. Default no-op so backends that
    /// don't support custom fonts compile without changes.
    fn set_fonts(&mut self, fonts: FontPack) {
        let _ = fonts;
    }

    /// Updates the status bar data shared with panels.
    ///
    /// Most editors will route this into a dedicated status-bar panel (the
    /// shell does not draw a status bar itself).
    fn set_status(&mut self, data: StatusBarData);

    /// Sets a shared [`EditorState`] reference. Panels typically grab this
    /// at construction; this method exists for shells that want to surface
    /// state to internal helpers (debug overlays, etc.).
    fn set_editor_state(&mut self, state: std::sync::Arc<std::sync::Mutex<EditorState>>);

    /// Renders the full editor frame.
    ///
    /// Iterates every registered slot and invokes
    /// [`EditorPanel::ui`] for each panel. The shell decides slot geometry;
    /// the panel decides slot content.
    fn show_frame(&mut self);
}
