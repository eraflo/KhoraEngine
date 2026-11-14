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

//! The restricted bar, the `TopBar` strip shown while the open project is
//! restricted. See the book: reference/editor.md, "11 — Restricted projects".

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{
    EditorPanel, EditorState, FontFamilyHint, Icon, TextAlign, UiBuilder, UiTheme,
};
use khora_tool_ui::widgets::paint::{icon, text, with_alpha};
use khora_tool_ui::widgets::{button, Button, ButtonKind};

/// The bar saying the open project is restricted.
///
/// The app registers it when the project turns restricted and removes it
/// when the project is trusted.
pub struct RestrictedBarPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl RestrictedBarPanel {
    /// The panel's id.
    pub const ID: &'static str = "khora.editor.restricted_bar";

    /// Creates a restricted bar over `state`.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self { state, theme }
    }
}

impl EditorPanel for RestrictedBarPanel {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Restricted mode"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(BAR_HEIGHT)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let t = &self.theme;
        let refusal = match self.state.lock() {
            Ok(state) => state.refusal.clone(),
            Err(_) => return,
        };
        let r = ui.panel_rect();
        ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], with_alpha(t.warning, 0.10), 0.0);
        ui.paint_line(
            [r[0], r[1] + r[3] - 1.0],
            [r[0] + r[2], r[1] + r[3] - 1.0],
            with_alpha(t.warning, 0.28),
            1.0,
        );

        let size = t.font_size_caption + 1.0;
        let y = r[1] + (r[3] - size) * 0.5 - 1.0;
        let mut x = r[0] + 14.0;
        icon(
            ui,
            [x, r[1] + (r[3] - 14.0) * 0.5],
            Icon::Lock,
            14.0,
            t.warning,
        );
        x += 22.0;
        let title = "Restricted mode";
        ui.paint_text_styled(
            [x, y],
            title,
            size,
            t.warning,
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        x += ui.measure_text(title, size, FontFamilyHint::Proportional)[0] + 12.0;
        let said = refusal.as_deref().unwrap_or(DESCRIPTION);
        text(ui, [x, y], said, size, t.text_dim);

        // The bar only asks: the app turns `confirm_trust` into
        // `EditorState::trust_confirm`, filled from the project's folder.
        let label = "Trust this project\u{2026}";
        let w = ui.measure_text(label, size, FontFamilyHint::Proportional)[0] + 28.0;
        let rect = [r[0] + r[2] - 14.0 - w, r[1] + (r[3] - 24.0) * 0.5, w, 24.0];
        let hit = button(
            ui,
            t,
            rect,
            "restricted-bar-trust",
            Button::new(label, ButtonKind::Ghost),
        );
        if hit.clicked {
            if let Ok(mut state) = self.state.lock() {
                state.pending_menu_action = Some("confirm_trust".to_owned());
            }
        }
    }
}

/// The bar's height.
const BAR_HEIGHT: f32 = 34.0;

/// What restricted means, said while nothing was refused.
const DESCRIPTION: &str = "This project's scripts, shaders and Build Game are off. Its files are \
                           read with stricter limits.";
