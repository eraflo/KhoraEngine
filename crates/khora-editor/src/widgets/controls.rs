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

use khora_sdk::editor_ui::{UiTheme, UiBuilder};

use super::paint::with_alpha;

/// A thin horizontal meter bar (used by DCC summary + agent rows).
pub fn paint_meter_bar(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    fraction: f32,
    fill_color: [f32; 4],
    theme: &UiTheme,
) {
    let h = 3.0;
    ui.paint_rect_filled(origin, [width, h], theme.background, 999.0);
    ui.paint_rect_stroke(
        origin,
        [width, h],
        with_alpha(theme.separator, 0.6),
        999.0,
        1.0,
    );
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.001 {
        ui.paint_rect_filled(origin, [width * f, h], fill_color, 999.0);
    }
}
