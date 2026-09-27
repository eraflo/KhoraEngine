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

//! Painting helpers shared by the control plane's views.

use khora_sdk::editor_ui::*;
use khora_sdk::{AgentImportance, ExecutionPhase};

use super::AgentSnapshot;
use khora_tool_ui::widgets::paint;

pub(super) fn phase_color_for(phase: ExecutionPhase, theme: &UiTheme) -> [f32; 4] {
    if phase == ExecutionPhase::INIT {
        theme.text_muted
    } else if phase == ExecutionPhase::OBSERVE {
        theme.accent_b
    } else if phase == ExecutionPhase::TRANSFORM {
        theme.accent_a
    } else if phase == ExecutionPhase::MUTATE {
        theme.warning
    } else if phase == ExecutionPhase::OUTPUT {
        theme.primary
    } else if phase == ExecutionPhase::FINALIZE {
        theme.success
    } else {
        theme.text
    }
}

pub(super) fn paint_card_box(
    ui: &mut dyn UiBuilder,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    icon: Icon,
    theme: &UiTheme,
) -> f32 {
    let header_h = 26.0;
    ui.paint_rect_filled(
        [x, y],
        [w, header_h],
        theme.surface_elevated,
        theme.radius_md,
    );
    paint::icon(ui, [x + 8.0, y + 7.0], icon, 12.0, theme.primary_dim);
    paint::text(ui, [x + 26.0, y + 7.0], title, 12.0, theme.text);
    y + header_h + 4.0
}

/// Render a key/value row with the key left-aligned and the value
/// right-aligned within `[x, x+w]`.
pub(super) fn kv(
    ui: &mut dyn UiBuilder,
    x: f32,
    y: f32,
    w: f32,
    key: &str,
    value: &str,
    theme: &UiTheme,
) {
    paint::text(ui, [x, y], key, 11.0, theme.text_dim);
    ui.paint_text_styled(
        [x + w - 4.0, y],
        value,
        11.0,
        theme.text,
        FontFamilyHint::Monospace,
        TextAlign::Right,
    );
}

impl AgentSnapshot {
    pub(super) fn importance_letter_label(&self) -> &'static str {
        match self.importance {
            AgentImportance::Critical => "Critical",
            AgentImportance::Important => "Important",
            AgentImportance::Optional => "Optional",
        }
    }
}
