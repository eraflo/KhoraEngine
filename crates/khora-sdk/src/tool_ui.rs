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

//! UI surface for standalone Khora tools (the hub, future asset
//! cookers, …).
//!
//! These tools depend on `khora-sdk` and reach the egui backend
//! exclusively through this module — never directly via `egui`
//! or `eframe`. The day the engine swaps backend, this re-export
//! list moves to whichever crate provides the new
//! [`run_native`] + [`AppContext`] implementation.
//!
//! This module is the **runtime** seam only. Khora's *look* — the brand
//! palette and the shared widget vocabulary — is cosmetics and lives in
//! the separate `khora-tool-ui` crate, which the SDK deliberately does
//! **not** depend on, so a game built on Khora never compiles the engine
//! vendor's brand. Tools depend on both.

pub use khora_core::math::{LinearRgba, Rect2D, Vec2};
pub use khora_core::ui::editor::{FontFamilyHint, Icon, InlineEditEvent, Interaction, TextAlign};
pub use khora_core::ui::{
    Align, Align2, App, AppContext, AppLifecycle, CornerRadius, FontHandle, FontPack, Margin,
    NamedFont, Stroke, UiBuilder, UiTheme,
};
pub use khora_infra::ui::egui::app::{run_native, WindowConfigInput, WindowIconInput};
