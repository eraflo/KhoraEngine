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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelLocation {
    /// Left sidebar (e.g. Scene Tree).
    Left,
    /// Right sidebar (e.g. Properties Inspector).
    Right,
    /// Bottom strip (e.g. Console, Asset Browser). Panels sharing this
    /// location are displayed as tabs.
    Bottom,
    /// Central area (e.g. 3D Viewport).
    Center,
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
}
