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

//! Abstract editor panel and dock location types.

use super::ui_builder::UiBuilder;

/// Where a panel is placed in the editor dock layout.
///
/// Slots are filled by panels registered with the [`EditorShell`](super::EditorShell).
/// The shell is responsible for laying them out — the panel only needs to know
/// which slot it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelLocation {
    /// Top fixed-height strip — non-resizable. Multiple `TopBar` panels stack
    /// vertically in registration order. Heights come from
    /// [`EditorPanel::preferred_size`].
    TopBar,
    /// Left fixed-width strip — non-resizable. Used for vertical mode
    /// switchers / "spines". Width comes from [`EditorPanel::preferred_size`].
    Spine,
    /// Resizable left sidebar (e.g. Scene Tree / Hierarchy). Default width
    /// comes from [`EditorPanel::preferred_size`] when set.
    Left,
    /// Resizable right sidebar (e.g. Inspector / Properties).
    Right,
    /// Resizable bottom strip (e.g. Console, Asset Browser). Panels sharing
    /// this slot are displayed as tabs.
    Bottom,
    /// Bottom fixed-height strip — non-resizable. Sits below the resizable
    /// `Bottom` slot. Used for status bars. Height comes from
    /// [`EditorPanel::preferred_size`].
    StatusBar,
    /// Central area (e.g. 3D Viewport).
    Center,
    /// Floating overlay rendered on top of the dock. Inner `i32` is the
    /// z-order — higher values draw on top. Used for command palettes,
    /// modal dialogs, etc.
    Floating(i32),
}

/// A single editor panel that can render itself into a [`UiBuilder`].
///
/// Panels are registered with an [`EditorShell`](super::EditorShell) at
/// startup. The shell calls [`ui()`](Self::ui) each frame for every visible
/// panel.
pub trait EditorPanel: Send + Sync {
    /// Unique identifier (used for dock serialization and lookup).
    fn id(&self) -> &str;

    /// Human-readable title shown in the tab / panel header.
    fn title(&self) -> &str;

    /// Build the panel contents for the current frame.
    fn ui(&mut self, ui: &mut dyn UiBuilder);

    /// Preferred size hint, in logical points.
    ///
    /// Interpretation depends on the panel's [`PanelLocation`]:
    ///
    /// | Location | Meaning |
    /// |---|---|
    /// | `TopBar`, `StatusBar` | Fixed height. |
    /// | `Spine` | Fixed width. |
    /// | `Left`, `Right` | Default width (still resizable by user). |
    /// | `Bottom` | Default height (still resizable by user). |
    /// | `Center`, `Floating` | Ignored. |
    ///
    /// Returning `None` lets the shell pick a sensible default for the slot.
    fn preferred_size(&self) -> Option<f32> {
        None
    }
}
