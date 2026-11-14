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

//! The editor's chrome: the branded strips around the dock, each an
//! [`EditorPanel`] the app registers at its [`PanelLocation`].
//! See the book: design/editor.md, "Anatomy".
//!
//! [`EditorPanel`]: khora_sdk::EditorPanel
//! [`PanelLocation`]: khora_sdk::PanelLocation

pub mod restricted_bar;
pub mod spine;
pub mod status_bar;
pub mod title_bar;

pub use restricted_bar::RestrictedBarPanel;
pub use spine::SpinePanel;
pub use status_bar::StatusBarPanel;
pub use title_bar::TitleBarPanel;

#[cfg(test)]
mod tests;
