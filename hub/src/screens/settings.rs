// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Settings — the GitHub account and the local engine clone.

use crate::AuthState;
use crate::HubApp;
use crate::Screen;
use crate::auth;
use khora_sdk::tool_ui::{FontFamilyHint, Icon, UiBuilder, UiTheme};
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{
    self, Button, ButtonKind, Tone, button, card, card_action_slot, chip,
    paint::{display, fill_stroke, mono, text},
};

const CONTENT_W: f32 = 620.0;

/// Renders the settings screen.
pub fn show_settings(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let t = khora_dark();
    let r = ui.panel_rect();
    let pad = 24.0;

    let x = r[0] + ((r[2] - CONTENT_W) * 0.5).max(pad);
    let w = CONTENT_W.min(r[2] - pad * 2.0);
    let mut y = r[1] + 20.0;

    if button(
        ui,
        &t,
        [x, y, 80.0, 26.0],
        "settings-back",
        Button::new("Back", ButtonKind::Ghost).icon(Icon::ArrowLeft),
    )
    .clicked
    {
        app.screen = Screen::Home;
    }
    y += 26.0 + 16.0;

    display(ui, [x, y], "Settings", 22.0, t.text);
    y += 34.0;

    y = github_card(app, ui, &t, [x, y, w, 132.0]) + 16.0;
    let _ = local_repo_card(app, ui, &t, [x, y, w, 176.0]);
}

/// Returns the bottom edge of the card it painted.
fn github_card(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) -> f32 {
    let body = card(ui, t, rect, Icon::Github, "GitHub");

    // Header-right: the account state, at a glance.
    let (label, tone) = match &app.settings.auth {
        AuthState::Connected { .. } => ("connected", Tone::Success),
        AuthState::Connecting { .. } => ("connecting", Tone::Warning),
        AuthState::Disconnected => ("not connected", Tone::Neutral),
    };
    let cw = ui.measure_text(label, t.font_size_caption, FontFamilyHint::Monospace)[0] + 34.0;
    let slot = card_action_slot(rect, cw);
    chip(ui, t, [slot[0], slot[1] + 4.0, cw, 18.0], label, tone, true);

    // Copy what we need out of the borrow before mutating `app` in the arms.
    enum View {
        Connected(String),
        Connecting(String),
        Disconnected,
    }
    let view = match &app.settings.auth {
        AuthState::Connected { login, .. } => View::Connected(login.clone()),
        AuthState::Connecting { message, .. } => View::Connecting(message.clone()),
        AuthState::Disconnected => View::Disconnected,
    };

    match view {
        View::Connected(login) => {
            // Avatar: the account's initials, not a fetched image — the hub
            // shouldn't reach the network to render a settings row.
            let initials: String = login.chars().take(2).collect();
            let cy = body[1] + 17.0;
            ui.paint_circle_filled([body[0] + 17.0, cy], 17.0, t.surface_interactive);
            ui.paint_text_styled(
                [body[0] + 17.0, cy - 7.0],
                &initials,
                13.0,
                t.primary,
                FontFamilyHint::Monospace,
                khora_sdk::tool_ui::TextAlign::Center,
            );

            text(
                ui,
                [body[0] + 45.0, body[1] + 4.0],
                &login,
                t.font_size_title,
                t.text,
            );
            mono(
                ui,
                [body[0] + 45.0, body[1] + 22.0],
                "device-flow token · stored 0600",
                t.font_size_caption,
                t.text_muted,
            );

            if button(
                ui,
                t,
                [widgets::right(body) - 110.0, body[1] + 2.0, 110.0, 30.0],
                "settings-gh-disconnect",
                Button::new("Disconnect", ButtonKind::Ghost),
            )
            .clicked
            {
                let _ = auth::forget_token();
                app.settings.auth = AuthState::Disconnected;
                app.banner = Some(crate::Banner::info("Disconnected from GitHub."));
            }
        }
        View::Connecting(message) => {
            // The device flow: the user has to type a code somewhere else, so
            // the code is the loudest thing on the card.
            text(
                ui,
                [body[0], body[1]],
                "Enter this code at github.com/login/device",
                t.font_size_caption + 1.0,
                t.text_muted,
            );
            mono(ui, [body[0], body[1] + 22.0], &message, 18.0, t.text);
        }
        View::Disconnected => {
            text(
                ui,
                [body[0], body[1] + 2.0],
                "Sign in to create GitHub repositories for new projects.",
                t.font_size_body,
                t.text_muted,
            );
            if button(
                ui,
                t,
                [body[0], body[1] + 24.0, 170.0, 32.0],
                "settings-gh-connect",
                Button::new("Connect GitHub", ButtonKind::Primary).icon(Icon::Github),
            )
            .clicked
            {
                app.start_github_auth();
            }
        }
    }

    widgets::bottom(rect)
}

/// Returns the bottom edge of the card it painted.
fn local_repo_card(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) -> f32 {
    let body = card(ui, t, rect, Icon::Hammer, "Local engine repository");

    text(
        ui,
        [body[0], body[1]],
        "Path to a local clone, used as the “dev” engine for new projects.",
        t.font_size_body,
        t.text_muted,
    );

    text(
        ui,
        [body[0], body[1] + 26.0],
        "Repository path",
        t.font_size_caption + 1.0,
        t.text_dim,
    );

    // The real text edit lives inside the painted frame.
    let field = [body[0], body[1] + 44.0, body[2] - 108.0, 32.0];
    fill_stroke(ui, field, t.background, t.border_strong, t.radius_md);
    ui.region_at(
        "settings-local-repo",
        [field[0] + 8.0, field[1] + 6.0, field[2] - 16.0, 20.0],
        &mut |ui| {
            ui.text_edit_singleline(&mut app.settings.local_repo_draft);
        },
    );

    if button(
        ui,
        t,
        [widgets::right(body) - 100.0, field[1], 100.0, 32.0],
        "settings-repo-browse",
        Button::new("Browse…", ButtonKind::Ghost).icon(Icon::FolderOpen),
    )
    .clicked
        && let Some(path) = rfd::FileDialog::new().pick_folder()
    {
        app.settings.local_repo_draft = path.to_string_lossy().to_string();
    }

    if button(
        ui,
        t,
        [body[0], field[1] + 42.0, 110.0, 32.0],
        "settings-repo-save",
        Button::new("Save path", ButtonKind::Primary),
    )
    .clicked
    {
        let value = app.settings.local_repo_draft.trim();
        app.config.local_engine_repo = (!value.is_empty()).then(|| value.to_owned());
        let _ = app.config.save();
        app.engine_manager.local_repo = app.config.local_engine_repo.clone().unwrap_or_default();
        app.banner = Some(crate::Banner::info("Local engine path saved."));
    }

    widgets::bottom(rect)
}
