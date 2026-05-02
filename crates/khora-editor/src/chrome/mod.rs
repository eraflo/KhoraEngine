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

//! Editor chrome — branded UI strips that surround the dock panels.
//!
//! These panels are owned entirely by the editor application. They contain
//! all Khora-specific branding (logo, project name, brand colors), all menu
//! definitions, and all status-bar formatting. The shell and the egui backend
//! know nothing about them — they are just regular [`EditorPanel`]
//! implementations that the editor registers at startup at the appropriate
//! [`PanelLocation`] slots (`TopBar`, `Spine`, `StatusBar`, …).
//!
//! ## Layout
//!
//! - [`TitleBarPanel`] — branded top strip with menus + Cmd+K search (44px).
//! - [`SpinePanel`] — vertical mode switcher on the far-left edge (56px).
//! - [`StatusBarPanel`] — bottom metrics strip (24px).
//!
//! Gizmo tool selection and Play/Stop transport controls live as overlays
//! inside the 3D viewport (see `panels::viewport::paint_tool_pill` /
//! `paint_transport_pill`), not as a top toolbar.
//!
//! [`EditorPanel`]: khora_sdk::EditorPanel
//! [`PanelLocation`]: khora_sdk::PanelLocation

pub mod spine;
pub mod status_bar;
pub mod title_bar;

pub use spine::SpinePanel;
pub use status_bar::StatusBarPanel;
pub use title_bar::TitleBarPanel;
