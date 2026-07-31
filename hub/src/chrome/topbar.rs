// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Top bar — brand lockup on the left, GitHub state on the right.

use crate::AuthState;
use crate::HubApp;
use crate::Screen;
use khora_sdk::tool_ui::UiBuilder;
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{
    self, Button, ButtonKind, Tone, brand_pill, chip, paint::vertical_gradient,
};

/// Height of the top bar, in logical points.
pub const TOPBAR_HEIGHT: f32 = 44.0;

/// Renders the top bar.
pub fn show_topbar(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let t = khora_dark();
    let r = ui.panel_rect();

    // The chrome lifts slightly toward the viewer, so the content below it
    // reads as the thing you're working on.
    vertical_gradient(ui, r, t.surface_elevated, t.surface, 6);
    ui.paint_line(
        [r[0], r[1] + r[3]],
        [widgets::right(r), r[1] + r[3]],
        t.border,
        1.0,
    );

    brand_pill(ui, &t, [r[0] + 16.0, r[1], 320.0, r[3]], app.screen_label());

    // ── Right cluster: where GitHub stands ──
    let cy = r[1] + r[3] * 0.5;
    let right = widgets::right(r) - 16.0;

    match &app.settings.auth {
        AuthState::Connected { login, .. } => {
            let label = format!("@{login}");
            let w = ui.measure_text(&label, t.font_size_caption, MONO)[0] + 34.0;
            chip(
                ui,
                &t,
                [right - w, cy - 9.0, w, 18.0],
                &label,
                Tone::Success,
                true,
            );
        }
        AuthState::Connecting { .. } => {
            let label = "connecting…";
            let w = ui.measure_text(label, t.font_size_caption, MONO)[0] + 34.0;
            chip(
                ui,
                &t,
                [right - w, cy - 9.0, w, 18.0],
                label,
                Tone::Warning,
                true,
            );
        }
        AuthState::Disconnected => {
            let (w, h) = (140.0, 28.0);
            let hit = widgets::button(
                ui,
                &t,
                [right - w, cy - h * 0.5, w, h],
                "topbar-connect-gh",
                Button::new("Connect GitHub", ButtonKind::Ghost)
                    .icon(khora_sdk::tool_ui::Icon::Github),
            );
            if hit.clicked {
                app.start_github_auth();
                app.screen = Screen::Settings;
            }
        }
    }
}

use khora_sdk::tool_ui::FontFamilyHint::Monospace as MONO;
