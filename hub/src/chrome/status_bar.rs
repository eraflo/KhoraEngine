// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Bottom status bar — Ready dot, project / engine counts, hub version,
//! GitHub connection state.

use crate::AuthState;
use crate::HubApp;
use crate::theme::{pal, tint};
use crate::widgets::{paint_diamond_filled, paint_v_hairline, rgba};
use khora_sdk::tool_ui::{FontFamilyHint, TextAlign, UiBuilder};

/// Render the 24px status bar.
pub fn show_status_bar(app: &HubApp, ui: &mut dyn UiBuilder) {
    let r = ui.panel_rect();
    ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], rgba(pal::SURFACE), 0.0);
    ui.paint_line(
        [r[0], r[1]],
        [r[0] + r[2], r[1]],
        rgba(tint(pal::SEPARATOR, 0.55)),
        1.0,
    );

    let cy = r[1] + r[3] * 0.5;
    let text_y = cy - 5.5;

    // ── Left cluster ──
    let mut x = r[0] + 14.0;
    paint_diamond_filled(ui, [x, cy], 4.0, pal::PRIMARY);
    x += 14.0;

    ui.paint_circle_filled([x, cy], 3.5, rgba(pal::SUCCESS));
    ui.paint_circle_filled([x, cy], 5.5, rgba(tint(pal::SUCCESS, 0.18)));
    x += 12.0;
    ui.paint_text_styled(
        [x, text_y],
        "Ready",
        11.0,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    x += 50.0;

    paint_v_hairline(ui, x, r[1] + 6.0, r[1] + r[3] - 6.0, tint(pal::SEPARATOR, 0.55));
    x += 12.0;
    let proj_label = format!("{} projects", app.config.recent_projects.len());
    ui.paint_text_styled(
        [x, text_y],
        &proj_label,
        11.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    x += 90.0;

    paint_v_hairline(ui, x, r[1] + 6.0, r[1] + r[3] - 6.0, tint(pal::SEPARATOR, 0.55));
    x += 12.0;
    let total_engines = app.config.engines.len() + usize::from(app.config.dev_engine().is_some());
    ui.paint_text_styled(
        [x, text_y],
        &format!("{total_engines} engines"),
        11.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    let _ = x;

    // ── Right cluster ──
    let mut rx_pos = r[0] + r[2] - 14.0;

    let version_label = format!("Hub v{}", env!("CARGO_PKG_VERSION"));
    let v_size = ui.measure_text(&version_label, 11.0, FontFamilyHint::Monospace);
    rx_pos -= v_size[0];
    ui.paint_text_styled(
        [rx_pos, text_y],
        &version_label,
        11.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    rx_pos -= 14.0;

    paint_v_hairline(
        ui,
        rx_pos,
        r[1] + 6.0,
        r[1] + r[3] - 6.0,
        tint(pal::SEPARATOR, 0.55),
    );
    rx_pos -= 12.0;
    let (gh_label, gh_color) = match &app.settings.auth {
        AuthState::Connected { login, .. } => (format!("GitHub @{login}"), pal::SUCCESS),
        AuthState::Connecting { .. } => ("GitHub: connecting".to_owned(), pal::WARNING),
        AuthState::Disconnected => ("GitHub: offline".to_owned(), pal::TEXT_MUTED),
    };
    let gh_size = ui.measure_text(&gh_label, 11.0, FontFamilyHint::Monospace);
    rx_pos -= gh_size[0];
    ui.paint_text_styled(
        [rx_pos, text_y],
        &gh_label,
        11.0,
        rgba(gh_color),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
}
