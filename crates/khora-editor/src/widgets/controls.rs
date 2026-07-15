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
//
//! Field-level controls used inside Inspector cards and panel toolbars.
//!
//! The meter bar itself lives in `khora_tool_ui::widgets`; this is a thin
//! adapter that keeps the editor's `(origin, width)` calling convention so the
//! panels don't all have to change. There is one implementation of the bar,
//! shared with the hub.

use khora_sdk::editor_ui::{UiBuilder, UiTheme};

/// A thin horizontal meter bar (used by the DCC summary + agent rows).
pub fn paint_meter_bar(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    fraction: f32,
    fill_color: [f32; 4],
    theme: &UiTheme,
) {
    khora_tool_ui::widgets::meter_bar(
        ui,
        theme,
        [origin[0], origin[1], width, 3.0],
        fraction,
        fill_color,
    );
}
