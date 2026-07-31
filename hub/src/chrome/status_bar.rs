// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Bottom status bar — a single monospace strip of facts.
//!
//! Everything here is data, so everything here is monospace: counts line up
//! between frames instead of jittering as digits change width.

use crate::AuthState;
use crate::HubApp;
use khora_sdk::tool_ui::{FontFamilyHint, UiBuilder};
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{self, Health, paint::mono, status_dot};

/// Height of the status bar, in logical points.
pub const STATUS_HEIGHT: f32 = 26.0;

/// Renders the status bar.
pub fn show_status_bar(app: &HubApp, ui: &mut dyn UiBuilder) {
    let t = khora_dark();
    let r = ui.panel_rect();

    ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], t.surface, 0.0);
    ui.paint_line([r[0], r[1]], [widgets::right(r), r[1]], t.border, 1.0);

    let size = t.font_size_caption;
    let y = widgets::text_y(r, size);
    let cy = r[1] + r[3] * 0.5;

    // ── Left: liveness, then the two counts ──
    let mut x = r[0] + 16.0;
    status_dot(ui, &t, [x, cy], Health::Good);
    x += 12.0;

    let projects = format!("{} projects", app.config.recent_projects.len());
    mono(ui, [x, y], &projects, size, t.text_dim);
    x += ui.measure_text(&projects, size, FontFamilyHint::Monospace)[0] + 14.0;

    separator(ui, &t, x, r);
    x += 14.0;

    let engines = app.config.engines.len() + usize::from(app.config.dev_engine().is_some());
    let engines = format!("{engines} engines");
    mono(ui, [x, y], &engines, size, t.text_dim);

    // ── Right: version, then GitHub state ──
    let mut rx = widgets::right(r) - 16.0;

    let version = format!("hub v{}", env!("CARGO_PKG_VERSION"));
    rx -= ui.measure_text(&version, size, FontFamilyHint::Monospace)[0];
    mono(ui, [rx, y], &version, size, t.text_muted);
    rx -= 14.0;

    separator(ui, &t, rx, r);
    rx -= 14.0;

    let (label, color) = match &app.settings.auth {
        AuthState::Connected { login, .. } => (format!("github @{login}"), t.success),
        AuthState::Connecting { .. } => ("github connecting".to_owned(), t.warning),
        AuthState::Disconnected => ("github offline".to_owned(), t.text_muted),
    };
    rx -= ui.measure_text(&label, size, FontFamilyHint::Monospace)[0];
    mono(ui, [rx, y], &label, size, color);
}

/// A short vertical rule between clusters.
fn separator(ui: &mut dyn UiBuilder, t: &khora_sdk::tool_ui::UiTheme, x: f32, r: [f32; 4]) {
    ui.paint_line([x, r[1] + 7.0], [x, r[1] + r[3] - 7.0], t.separator, 1.0);
}
