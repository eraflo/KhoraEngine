// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Engine manager — the local dev clone, what you can download, what you have.

use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use khora_sdk::tool_ui::{FontFamilyHint, Icon, UiBuilder, UiTheme};
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{
    self, Button, ButtonKind, Tone, button, card, card_action_slot, chip, empty_state, icon_button,
    paint::{display, fill_stroke, mono, text},
    progress_row, skeleton,
};

const CONTENT_W: f32 = 760.0;
const ROW_H: f32 = 44.0;

/// Renders the engine manager.
pub fn show_engine_manager(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    if !app.engine_manager.has_fetched_once && app.engine_manager.fetch_rx.is_none() {
        app.engine_manager.fetching = true;
        app.engine_manager.has_fetched_once = true;
        app.engine_manager.fetch_rx = Some(github::fetch_releases_async());
    }

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
        "em-back",
        Button::new("Back", ButtonKind::Ghost).icon(Icon::ArrowLeft),
    )
    .clicked
    {
        app.screen = Screen::Home;
    }
    y += 26.0 + 16.0;

    display(ui, [x, y], "Engines", 22.0, t.text);
    y += 36.0;

    y = local_card(app, ui, &t, [x, y, w, 96.0]) + 16.0;
    y = releases_card(app, ui, &t, [x, y, w, releases_height(app)]) + 16.0;
    let _ = installed_card(app, ui, &t, [x, y, w, installed_height(app)]);

    if let Some(version) = app.engine_manager.uninstall_confirm.clone() {
        confirm_uninstall(app, ui, &t, &version);
    }
}

// ── Local dev engine ────────────────────────────────────

fn local_card(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) -> f32 {
    let body = card(ui, t, rect, Icon::Hammer, "Local dev engine");
    let dev = app.config.dev_engine();

    let (label, tone) = if dev.is_some() {
        ("detected", Tone::Success)
    } else {
        ("not configured", Tone::Neutral)
    };
    let cw = ui.measure_text(label, t.font_size_caption, FontFamilyHint::Monospace)[0] + 34.0;
    let slot = card_action_slot(rect, cw);
    chip(ui, t, [slot[0], slot[1] + 4.0, cw, 18.0], label, tone, true);

    match dev {
        Some(d) => {
            mono(
                ui,
                [body[0], body[1]],
                &d.editor_binary,
                t.font_size_caption,
                t.text_muted,
            );
        }
        None => {
            text(
                ui,
                [body[0], body[1]],
                "Set a local engine repository in Settings to enable the “dev” engine.",
                t.font_size_body,
                t.text_muted,
            );
        }
    }
    widgets::bottom(rect)
}

// ── Available releases ──────────────────────────────────

fn releases_height(app: &HubApp) -> f32 {
    let head = 40.0 + 32.0;
    if app.engine_manager.fetching {
        return head + 66.0;
    }
    if app.engine_manager.fetch_error.is_some() {
        return head + 56.0;
    }
    let n = app.engine_manager.releases.len().clamp(1, 4) as f32;
    head + n * (ROW_H + 6.0)
        + if app.engine_manager.download_progress.is_some() {
            34.0
        } else {
            0.0
        }
}

fn releases_card(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) -> f32 {
    let body = card(ui, t, rect, Icon::Download, "Available releases");

    // Header-right: refresh.
    let slot = card_action_slot(rect, 96.0);
    if button(
        ui,
        t,
        slot,
        "em-refresh",
        Button::new("Refresh", ButtonKind::Ghost)
            .icon(Icon::Refresh)
            .enabled(!app.engine_manager.fetching),
    )
    .clicked
        && app.engine_manager.fetch_rx.is_none()
    {
        app.engine_manager.fetching = true;
        app.engine_manager.fetch_error = None;
        app.engine_manager.fetch_rx = Some(github::fetch_releases_async());
    }

    let mut y = body[1];

    // Loading: show the shape of what's coming, not a spinner.
    if app.engine_manager.fetching {
        skeleton(ui, t, [body[0], y, body[2], 60.0], 3);
        return widgets::bottom(rect);
    }

    // Failure: say what broke, and offer the way out.
    if let Some(err) = app.engine_manager.fetch_error.clone() {
        let row = [body[0], y, body[2], 40.0];
        fill_stroke(
            ui,
            row,
            t.surface_elevated,
            widgets::tint(t.error, 0.35),
            t.radius_md,
        );

        widgets::paint::icon(
            ui,
            [row[0] + 12.0, row[1] + 12.0],
            Icon::Warn,
            15.0,
            t.error,
        );
        text(
            ui,
            [row[0] + 34.0, row[1] + 13.0],
            "Couldn't fetch releases",
            t.font_size_body,
            t.text,
        );

        if button(
            ui,
            t,
            [widgets::right(row) - 90.0, row[1] + 6.0, 80.0, 28.0],
            "em-retry",
            Button::new("Retry", ButtonKind::Ghost).icon(Icon::Refresh),
        )
        .clicked
        {
            app.engine_manager.fetch_error = None;
            app.engine_manager.fetching = true;
            app.engine_manager.fetch_rx = Some(github::fetch_releases_async());
        }

        mono(
            ui,
            [row[0], widgets::bottom(row) + 6.0],
            &err,
            t.font_size_caption,
            t.text_disabled,
        );
        return widgets::bottom(rect);
    }

    if app.engine_manager.releases.is_empty() {
        text(
            ui,
            [body[0], y],
            "No releases published yet.",
            t.font_size_body,
            t.text_muted,
        );
        return widgets::bottom(rect);
    }

    let installed: std::collections::HashSet<String> = app
        .config
        .engines
        .iter()
        .map(|e| e.version.clone())
        .collect();

    let releases = app.engine_manager.releases.clone();
    let mut to_download: Option<github::GithubRelease> = None;

    for release in releases.iter().take(4) {
        let already = installed.contains(&release.tag_name);
        release_row(
            ui,
            t,
            [body[0], y, body[2], ROW_H],
            release,
            already,
            &mut to_download,
        );
        y += ROW_H + 6.0;
    }

    if let Some((done, total)) = app.engine_manager.download_progress {
        let ratio = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };
        progress_row(
            ui,
            t,
            [body[0], y, body[2], 22.0],
            "Downloading engine",
            ratio,
        );
    }

    if let Some(release) = to_download
        && let Some(asset) = release.editor_asset()
    {
        let runtime = release.runtime_asset();
        app.engine_manager.download_progress = Some((0, asset.size));
        app.engine_manager.download_rx =
            Some(download::start_download(asset, runtime, &release.tag_name));
    }

    widgets::bottom(rect)
}

fn release_row(
    ui: &mut dyn UiBuilder,
    t: &UiTheme,
    rect: [f32; 4],
    release: &github::GithubRelease,
    installed: bool,
    to_download: &mut Option<github::GithubRelease>,
) {
    fill_stroke(ui, rect, t.background, t.border, t.radius_md);

    let pad = 12.0;
    let mut x = rect[0] + pad;

    mono(
        ui,
        [x, widgets::text_y(rect, t.font_size_body)],
        &release.tag_name,
        t.font_size_body,
        t.text,
    );
    x += ui.measure_text(
        &release.tag_name,
        t.font_size_body,
        FontFamilyHint::Monospace,
    )[0] + 12.0;

    if release.prerelease {
        let w = 78.0;
        chip(
            ui,
            t,
            [x, rect[1] + (rect[3] - 18.0) * 0.5, w, 18.0],
            "pre-release",
            Tone::Warning,
            true,
        );
        x += w + 8.0;
    }
    if installed {
        let w = 68.0;
        chip(
            ui,
            t,
            [x, rect[1] + (rect[3] - 18.0) * 0.5, w, 18.0],
            "installed",
            Tone::Success,
            true,
        );
    }

    // Right: size, then the action.
    if installed {
        return; // Uninstall lives in the Installed card — one place per action.
    }

    if let Some(asset) = release.editor_asset() {
        let size = format!("{} MB", asset.size / 1_000_000);
        let sw = ui.measure_text(&size, t.font_size_caption, FontFamilyHint::Monospace)[0];
        mono(
            ui,
            [
                widgets::right(rect) - pad - 106.0 - sw - 12.0,
                widgets::text_y(rect, t.font_size_caption),
            ],
            &size,
            t.font_size_caption,
            t.text_muted,
        );

        if button(
            ui,
            t,
            [
                widgets::right(rect) - pad - 106.0,
                rect[1] + (rect[3] - 28.0) * 0.5,
                106.0,
                28.0,
            ],
            &format!("em-dl-{}", release.tag_name),
            Button::new("Download", ButtonKind::Ghost).icon(Icon::Download),
        )
        .clicked
        {
            *to_download = Some(release.clone());
        }
    } else {
        mono(
            ui,
            [
                widgets::right(rect) - pad - 100.0,
                widgets::text_y(rect, t.font_size_caption),
            ],
            "no asset",
            t.font_size_caption,
            t.text_disabled,
        );
    }
}

// ── Installed ───────────────────────────────────────────

fn installed_height(app: &HubApp) -> f32 {
    let head = 40.0 + 32.0;
    if app.config.engines.is_empty() {
        return head + 120.0;
    }
    head + app.config.engines.len() as f32 * (ROW_H + 6.0)
}

fn installed_card(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) -> f32 {
    let body = card(ui, t, rect, Icon::Package, "Installed");

    if app.config.engines.is_empty() {
        empty_state(
            ui,
            t,
            [body[0], body[1], body[2], 110.0],
            Icon::Package,
            "No engines installed",
            "Download one from the list above.",
        );
        return widgets::bottom(rect);
    }

    let engines = app.config.engines.clone();
    let mut y = body[1];
    let mut ask: Option<String> = None;

    for engine in engines.iter() {
        let row = [body[0], y, body[2], ROW_H];
        fill_stroke(ui, row, t.background, t.border, t.radius_md);

        let pad = 12.0;
        mono(
            ui,
            [row[0] + pad, row[1] + 8.0],
            &engine.version,
            t.font_size_body,
            t.text,
        );
        mono(
            ui,
            [row[0] + pad, row[1] + 25.0],
            &engine.editor_binary,
            t.font_size_caption - 0.5,
            t.text_disabled,
        );

        if icon_button(
            ui,
            t,
            [
                widgets::right(row) - pad - 28.0,
                row[1] + (ROW_H - 28.0) * 0.5,
                28.0,
                28.0,
            ],
            &format!("em-rm-{}", engine.version),
            Icon::Trash,
            true,
        )
        .clicked
        {
            ask = Some(engine.version.clone());
        }

        y += ROW_H + 6.0;
    }

    if let Some(v) = ask {
        app.engine_manager.uninstall_confirm = Some(v);
    }

    widgets::bottom(rect)
}

// ── Destructive confirm ─────────────────────────────────

fn confirm_uninstall(app: &mut HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, version: &str) {
    let mut go = false;
    let mut close = false;

    ui.modal("em-uninstall-confirm", [420.0, 180.0], &mut |ui| {
        let r = ui.panel_rect();
        let pad = 20.0;
        let x = r[0] + pad;

        text(
            ui,
            [x, r[1] + pad],
            &format!("Uninstall engine {version}?"),
            t.font_size_title,
            t.text,
        );
        text(
            ui,
            [x, r[1] + pad + 28.0],
            "This deletes the engine from disk. Projects pinned to it will",
            t.font_size_caption + 1.0,
            t.text_muted,
        );
        text(
            ui,
            [x, r[1] + pad + 44.0],
            "report a missing engine until you reinstall or re-pin them.",
            t.font_size_caption + 1.0,
            t.text_muted,
        );

        let by = widgets::bottom(r) - pad - 32.0;
        let bw = 100.0;
        if button(
            ui,
            t,
            [widgets::right(r) - pad - bw, by, bw, 32.0],
            "em-confirm-rm",
            Button::new("Uninstall", ButtonKind::Danger).icon(Icon::Trash),
        )
        .clicked
        {
            go = true;
        }
        if button(
            ui,
            t,
            [widgets::right(r) - pad - bw * 2.0 - 8.0, by, bw, 32.0],
            "em-confirm-cancel",
            Button::new("Cancel", ButtonKind::Ghost),
        )
        .clicked
        {
            close = true;
        }
    });

    if go {
        uninstall_version(app, version);
        app.engine_manager.uninstall_confirm = None;
    } else if close {
        app.engine_manager.uninstall_confirm = None;
    }
}

/// Drops the engine from config and wipes its install directory.
fn uninstall_version(app: &mut HubApp, version: &str) {
    if let Some(idx) = app.config.engines.iter().position(|e| e.version == version) {
        app.config.engines.remove(idx);
        let _ = app.config.save();
    }
    match download::uninstall_engine(version) {
        Ok(()) => {
            app.banner = Some(crate::Banner::info(format!(
                "Uninstalled engine {version}."
            )))
        }
        Err(e) => {
            app.banner = Some(crate::Banner::error(format!(
                "Couldn't remove engine {version} from disk: {e}"
            )))
        }
    }
}
