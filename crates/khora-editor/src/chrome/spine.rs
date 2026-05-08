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

//! Spine — vertical mode switcher on the far-left edge.
//!
//! Phase C: real Lucide icons, silver bar + glow on the active button,
//! tooltips on hover. Six modes mirroring the design mockup.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::brand::paint_diamond_filled;
use crate::widgets::paint::{paint_hairline_h, paint_icon, with_alpha};

const SPINE_WIDTH: f32 = 56.0;
const BTN_SIZE: f32 = 40.0;

/// Modes that have a real workspace wired up. The other variants of
/// `EditorMode` (Canvas2D / NodeGraph / Animation / Shader) exist in the
/// state enum for future use but are NOT exposed here — adding them back
/// is a follow-up once each workspace is implemented.
const MODES: &[(EditorMode, Icon, &str)] = &[
    (EditorMode::Scene, Icon::Cube, "Scene"),
    (EditorMode::ControlPlane, Icon::Cpu, "Control Plane · DCC"),
];

/// Bottom items are also pruned — Plugins and Preferences had no handler.
/// Re-add them when there's actually something to open.
const BOTTOM_ITEMS: &[(Icon, &str)] = &[];

/// Vertical mode-switcher strip — 56px wide.
pub struct SpinePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl SpinePanel {
    /// Creates a new spine.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self { state, theme }
    }

    fn current_mode(&self) -> EditorMode {
        self.state
            .lock()
            .ok()
            .map(|s| s.active_mode)
            .unwrap_or_default()
    }

    fn set_mode(&self, mode: EditorMode) {
        if let Ok(mut s) = self.state.lock() {
            s.active_mode = mode;
        }
    }
}

impl EditorPanel for SpinePanel {
    fn id(&self) -> &str {
        "khora.editor.spine"
    }

    fn title(&self) -> &str {
        "Spine"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(SPINE_WIDTH)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = &self.theme;
        let rect = ui.panel_rect();
        let [px, py, pw, ph] = rect;

        ui.paint_rect_filled([px, py], [pw, ph], theme.background, 0.0);
        ui.paint_line(
            [px + pw, py],
            [px + pw, py + ph],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        // ── Brand block ────────────────────────────────
        let brand_cy = py + 26.0;
        let brand_cx = px + pw * 0.5;
        ui.paint_rect_filled(
            [brand_cx - 20.0, brand_cy - 20.0],
            [40.0, 40.0],
            theme.surface_active,
            theme.radius_md,
        );
        ui.paint_rect_stroke(
            [brand_cx - 20.0, brand_cy - 20.0],
            [40.0, 40.0],
            theme.border,
            theme.radius_md,
            1.0,
        );
        paint_diamond_filled(ui, brand_cx, brand_cy, 10.0, theme.primary);

        paint_hairline_h(ui, px + 14.0, py + 56.0, pw - 28.0, theme.separator);

        // ── Mode buttons ───────────────────────────────
        let current = self.current_mode();
        let mut cy = py + 70.0;

        for (mode, icon, tooltip) in MODES {
            paint_spine_button(
                ui,
                px,
                cy,
                pw,
                *icon,
                *mode == current,
                tooltip,
                &format!("spine-{}", tooltip),
                theme,
                |selected| {
                    if selected {
                        self.set_mode(*mode);
                    }
                },
            );
            cy += BTN_SIZE + 4.0;
        }

        // ── Bottom items ───────────────────────────────
        let mut by = py + ph - 6.0 - BTN_SIZE * BOTTOM_ITEMS.len() as f32 - 4.0;
        for (icon, tooltip) in BOTTOM_ITEMS {
            paint_spine_button(
                ui,
                px,
                by,
                pw,
                *icon,
                false,
                tooltip,
                &format!("spine-{}", tooltip),
                theme,
                |_| {},
            );
            by += BTN_SIZE + 4.0;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_spine_button(
    ui: &mut dyn UiBuilder,
    px: f32,
    cy: f32,
    pw: f32,
    icon: Icon,
    active: bool,
    tooltip: &str,
    id_salt: &str,
    theme: &UiTheme,
    on_click: impl FnOnce(bool),
) {
    let bx = px + (pw - BTN_SIZE) * 0.5;
    let interaction = ui.interact_rect(id_salt, [bx, cy, BTN_SIZE, BTN_SIZE]);

    if active || interaction.hovered {
        ui.paint_rect_filled(
            [bx, cy],
            [BTN_SIZE, BTN_SIZE],
            theme.surface_elevated,
            theme.radius_md,
        );
    }

    if active {
        // Vertical accent bar on the left + soft glow
        ui.paint_rect_filled(
            [bx - 1.0, cy + 8.0],
            [2.0, BTN_SIZE - 16.0],
            theme.primary,
            1.0,
        );
        ui.paint_rect_filled(
            [bx - 4.0, cy + 6.0],
            [4.0, BTN_SIZE - 12.0],
            with_alpha(theme.primary, 0.25),
            2.0,
        );
    }

    let icon_color = if active {
        theme.primary
    } else if interaction.hovered {
        theme.text
    } else {
        theme.text_dim
    };
    paint_icon(ui, [bx + 12.0, cy + 12.0], icon, 16.0, icon_color);

    if interaction.hovered {
        ui.tooltip_for_last(tooltip);
    }
    if interaction.clicked {
        on_click(true);
    }
}
