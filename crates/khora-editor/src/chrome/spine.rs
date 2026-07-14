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

//! Spine — the vertical workspace switcher on the far-left edge.
//!
//! The active workspace is marked in **gold**, because which workspace you are
//! in *is* a selection — the same thing the gold bar means everywhere else in
//! the editor. (The hub's sidebar is different: it navigates between places,
//! so it tints with the brand silver instead.)

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_tool_ui::widgets::paint::{icon_centered, selection_bar};

/// Width of the spine strip.
pub const SPINE_WIDTH: f32 = 48.0;
const BTN_SIZE: f32 = 34.0;

/// The workspaces that ship today.
const MODES: &[(EditorMode, Icon, &str)] = &[
    (EditorMode::Scene, Icon::Cube, "Scene"),
    (EditorMode::ControlPlane, Icon::Cpu, "Control Plane · DCC"),
];

/// Vertical workspace-switcher strip.
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
        let [px, py, pw, ph] = ui.panel_rect();

        ui.paint_rect_filled([px, py], [pw, ph], theme.surface, 0.0);
        ui.paint_line([px + pw, py], [px + pw, py + ph], theme.border, 1.0);

        // Brand mark — the diamond, no plate around it. The spine is chrome;
        // it should recede, not compete with the work.
        let cx = px + pw * 0.5;
        khora_tool_ui::widgets::diamond(ui, [cx, py + 22.0], 18.0, theme.primary);

        // Mode buttons.
        let current = self.current_mode();
        let mut y = py + 48.0;

        for (mode, icon, tooltip) in MODES {
            let bx = px + (pw - BTN_SIZE) * 0.5;
            let rect = [bx, y, BTN_SIZE, BTN_SIZE];
            let active = *mode == current;

            let hit = ui.interact_rect(&format!("spine-{tooltip}"), rect);

            if active || hit.hovered {
                ui.paint_rect_filled(
                    [rect[0], rect[1]],
                    [rect[2], rect[3]],
                    theme.surface_interactive,
                    theme.radius_md,
                );
            }
            if active {
                // The gold bar sits in the gutter, left of the button.
                selection_bar(ui, [px + 1.0, y, 2.0, BTN_SIZE], theme.accent_c);
            }

            let color = if active {
                theme.accent_c
            } else if hit.hovered {
                theme.text
            } else {
                theme.text_muted
            };
            icon_centered(ui, rect, *icon, 17.0, color);

            if hit.hovered {
                ui.tooltip_for_last(tooltip);
            }
            if hit.clicked {
                self.set_mode(*mode);
            }

            y += BTN_SIZE + 4.0;
        }
    }
}
