// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Top bar — brand pill on the left, GitHub status / connect on the right.

use crate::AuthState;
use crate::HubApp;
use crate::Screen;
use crate::theme::{pal, tint};
use crate::widgets::{paint_diamond_filled, paint_v_hairline, paint_vertical_gradient, rgba};
use khora_sdk::tool_ui::{FontFamilyHint, LinearRgba, TextAlign, UiBuilder};

use super::banner::paint_chip_at;

/// Render the 44px top bar.
pub fn show_topbar(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let r = ui.panel_rect();
    paint_vertical_gradient(ui, r, pal::SURFACE, pal::BG, 6);
    ui.paint_line(
        [r[0], r[1] + r[3]],
        [r[0] + r[2], r[1] + r[3]],
        rgba(tint(pal::BORDER, 0.55)),
        1.0,
    );

    paint_brand_pill(app, ui, r);

    // Right cluster: auth state.
    let right_x = r[0] + r[2];
    let cy = r[1] + r[3] * 0.5;
    match &app.settings.auth {
        AuthState::Connected { login, .. } => {
            let label = format!("@{login}");
            let w = ui.measure_text(&label, 11.0, FontFamilyHint::Proportional)[0] + 30.0;
            let pos = [right_x - w - 14.0, cy - 11.0];
            paint_chip_at(ui, pos, &label, pal::SUCCESS);
        }
        AuthState::Connecting { .. } => {
            let label = "Connecting…";
            let w = ui.measure_text(label, 11.0, FontFamilyHint::Proportional)[0] + 30.0;
            let pos = [right_x - w - 14.0, cy - 11.0];
            paint_chip_at(ui, pos, label, pal::WARNING);
        }
        AuthState::Disconnected => {
            let label = "Connect GitHub";
            let w = 140.0;
            let h = 26.0;
            let pos = [right_x - w - 14.0, cy - h * 0.5];
            let int = ui.interact_rect("topbar-connect-gh", [pos[0], pos[1], w, h]);
            let fill = if int.hovered {
                LinearRgba::new(
                    (pal::SURFACE3.r * 1.08).min(1.0),
                    (pal::SURFACE3.g * 1.08).min(1.0),
                    (pal::SURFACE3.b * 1.08).min(1.0),
                    pal::SURFACE3.a,
                )
            } else {
                pal::SURFACE3
            };
            ui.paint_rect_filled(pos, [w, h], rgba(fill), 5.0);
            ui.paint_rect_stroke(pos, [w, h], rgba(pal::BORDER), 5.0, 1.0);
            let text_size = ui.measure_text(label, 12.0, FontFamilyHint::Proportional);
            ui.paint_text_styled(
                [pos[0] + (w - text_size[0]) * 0.5, pos[1] + (h - text_size[1]) * 0.5],
                label,
                12.0,
                rgba(pal::TEXT_DIM),
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            if int.clicked {
                app.start_github_auth();
                app.screen = Screen::Settings;
            }
        }
    }
}

fn paint_brand_pill(app: &HubApp, ui: &mut dyn UiBuilder, panel_rect: [f32; 4]) {
    let total_w = 220.0;
    let height = 26.0;
    let pos = [panel_rect[0] + 14.0, panel_rect[1] + (panel_rect[3] - height) * 0.5];
    let pill_radius = (height * 0.5).floor();

    ui.paint_rect_filled(pos, [total_w, height], rgba(tint(pal::BG, 0.55)), pill_radius);
    ui.paint_rect_stroke(
        pos,
        [total_w, height],
        rgba(tint(pal::BORDER, 0.6)),
        pill_radius,
        1.0,
    );

    let cy = pos[1] + height * 0.5;
    paint_diamond_filled(ui, [pos[0] + 14.0, cy], 6.5, pal::PRIMARY);
    ui.paint_text_styled(
        [pos[0] + 26.0, cy - 6.5],
        "Khora",
        13.0,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    let sep_x = pos[0] + 80.0;
    paint_v_hairline(
        ui,
        sep_x,
        pos[1] + 5.0,
        pos[1] + height - 5.0,
        tint(pal::BORDER, 0.7),
    );

    ui.paint_text_styled(
        [sep_x + 8.0, cy - 6.0],
        app.screen_label(),
        12.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}
