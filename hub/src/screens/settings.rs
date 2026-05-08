// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Settings screen — GitHub auth + local engine path.

use crate::AuthState;
use crate::HubApp;
use crate::Screen;
use crate::auth;
use crate::theme::{pal, tint};
use crate::widgets::{ghost_button, paint_separator, primary_button, rgba, status_chip};
use khora_sdk::tool_ui::{
    CornerRadius, FontFamilyHint, Margin, Stroke, TextAlign, UiBuilder,
};

pub fn show_settings(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    ui.spacing(20.0);
    ui.indent("settings_root", &mut |ui| {
        ui.horizontal(&mut |row| {
            if ghost_button(row, "settings-back", "< Back", [70.0, 26.0]).clicked {
                app.screen = Screen::Home;
            }
        });
        ui.spacing(8.0);
        title_label(ui, "Settings", 20.0);
        ui.colored_label(
            rgba(pal::TEXT_DIM),
            "GitHub account, local engine path, and more.",
        );
        ui.spacing(20.0);
        paint_separator(ui, tint(pal::SEPARATOR, 0.55));
        ui.spacing(14.0);

        ui.scroll_area("settings_scroll", &mut |ui| {
            show_github_card(app, ui);
            ui.spacing(20.0);
            show_local_repo_card(app, ui);
        });
    });
}

fn show_github_card(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    card_frame(ui, "GitHub", &mut |ui| {
        ui.spacing(8.0);
        ui.colored_label(
            rgba(pal::TEXT_DIM),
            "Sign in via the GitHub Device Flow to create remote repos for new projects.",
        );
        ui.spacing(12.0);

        match &app.settings.auth {
            AuthState::Connected { login, .. } => {
                status_chip(ui, &format!("Connected as @{login}"), pal::SUCCESS);
                ui.spacing(8.0);
                if ghost_button(ui, "settings-gh-disconnect", "Disconnect", [120.0, 28.0]).clicked {
                    let _ = auth::forget_token();
                    app.settings.auth = AuthState::Disconnected;
                    app.banner = Some(crate::Banner::info("Disconnected from GitHub."));
                }
            }
            AuthState::Connecting { message, .. } => {
                let m = message.clone();
                status_chip(ui, "Connecting…", pal::WARNING);
                ui.spacing(6.0);
                ui.colored_label(rgba(pal::TEXT_DIM), &m);
            }
            AuthState::Disconnected => {
                status_chip(ui, "Disconnected", pal::TEXT_MUTED);
                ui.spacing(8.0);
                if primary_button(ui, "settings-gh-connect", "Connect GitHub", [180.0, 32.0]).clicked
                {
                    app.start_github_auth();
                }
            }
        }
    });
}

fn show_local_repo_card(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    card_frame(ui, "Local engine repository", &mut |ui| {
        ui.spacing(8.0);
        ui.colored_label(
            rgba(pal::TEXT_DIM),
            "Path to a local clone of the KhoraEngine repository (used for the 'dev' engine choice).",
        );
        ui.spacing(10.0);

        ui.horizontal(&mut |row| {
            row.label("Path:");
            row.text_edit_singleline(&mut app.settings.local_repo_draft);
        });
        ui.spacing(8.0);
        ui.horizontal(&mut |row| {
            if ghost_button(row, "settings-repo-browse", "Browse…", [110.0, 28.0]).clicked
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                app.settings.local_repo_draft = path.to_string_lossy().to_string();
            }
            if primary_button(row, "settings-repo-save", "Save Path", [120.0, 32.0]).clicked {
                let value = app.settings.local_repo_draft.trim();
                if value.is_empty() {
                    app.config.local_engine_repo = None;
                } else {
                    app.config.local_engine_repo = Some(value.to_owned());
                }
                let _ = app.config.save();
                app.engine_manager.local_repo = app
                    .config
                    .local_engine_repo
                    .clone()
                    .unwrap_or_default();
                app.banner = Some(crate::Banner::info("Local engine path saved."));
            }
        });
    });
}

/// Auto-sized card frame backed by [`UiBuilder::frame_box`]. Title is
/// rendered as the first child of the frame so it stays inside.
fn card_frame(ui: &mut dyn UiBuilder, title: &str, body: &mut dyn FnMut(&mut dyn UiBuilder)) {
    ui.frame_box(
        Margin::same(14.0),
        Some(pal::SURFACE2),
        Stroke::new(pal::BORDER, 1.0),
        CornerRadius::same(6.0),
        &mut |ui| {
            let pos = ui.cursor_pos();
            ui.paint_rect_filled([pos[0], pos[1] + 2.0], [3.0, 18.0], rgba(pal::PRIMARY_DIM), 2.0);
            ui.paint_text_styled(
                [pos[0] + 12.0, pos[1] + 2.0],
                title,
                14.0,
                rgba(pal::TEXT),
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            ui.spacing(28.0);
            body(ui);
        },
    );
}

fn title_label(ui: &mut dyn UiBuilder, text: &str, size: f32) {
    let pos = ui.cursor_pos();
    ui.paint_text_styled(
        pos,
        text,
        size,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.spacing(size + 8.0);
}
