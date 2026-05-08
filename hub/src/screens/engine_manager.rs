// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Engine Manager screen — local repo + GitHub releases + downloads.
//!
//! Visual language stays aligned with the editor's design: cards with a
//! 3-px primary stripe + diamond, status-coloured accents per engine
//! state, badges for `pre`/`installed`/`current`.

use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use crate::theme::{pal, tint};
use crate::widgets::{
    badge, ghost_button, paint_diamond_filled, paint_separator, primary_button, rgba,
};
use khora_sdk::tool_ui::{
    CornerRadius, FontFamilyHint, Margin, Stroke, TextAlign, UiBuilder,
};

pub fn show_engine_manager(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    if !app.engine_manager.has_fetched_once && app.engine_manager.fetch_rx.is_none() {
        app.engine_manager.fetching = true;
        app.engine_manager.has_fetched_once = true;
        app.engine_manager.fetch_rx = Some(github::fetch_releases_async());
    }

    ui.spacing(20.0);
    ui.indent("em_root", &mut |ui| {
        ui.horizontal(&mut |row| {
            if ghost_button(row, "em-back", "< Back", [70.0, 26.0]).clicked {
                app.screen = Screen::Home;
            }
        });
        ui.spacing(10.0);
        title_label(ui, "Engine Manager", 22.0);
        ui.colored_label(rgba(pal::TEXT_DIM), "Manage local + downloaded engine builds.");
        ui.spacing(20.0);
        paint_separator(ui, tint(pal::SEPARATOR, 0.55));
        ui.spacing(14.0);

        ui.scroll_area("em_scroll", &mut |ui| {
            show_local_card(app, ui);
            ui.spacing(20.0);
            show_releases_card(app, ui);
            ui.spacing(20.0);
            show_installed_card(app, ui);
        });
    });
}

fn show_local_card(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let dev = app.config.dev_engine();
    let title = if dev.is_some() {
        "Local engine (dev) — detected"
    } else {
        "Local engine (dev) — not configured"
    };
    let accent = if dev.is_some() { pal::SUCCESS } else { pal::WARNING };
    card_frame(ui, title, accent, &mut |ui| match dev.as_ref() {
        Some(d) => {
            kv_row(ui, "Editor", &d.editor_binary, FontFamilyHint::Monospace);
            kv_row(ui, "Version", &d.version, FontFamilyHint::Proportional);
        }
        None => {
            ui.colored_label(
                rgba(pal::TEXT_DIM),
                "Set the local engine repository in Settings to enable the 'dev' engine.",
            );
        }
    });
}

fn show_releases_card(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    card_frame(ui, "Available releases", pal::PRIMARY, &mut |ui| {
        // Toolbar.
        ui.horizontal(&mut |row| {
            if ghost_button(row, "em-refresh", "Refresh", [110.0, 28.0]).clicked
                && app.engine_manager.fetch_rx.is_none()
            {
                app.engine_manager.fetching = true;
                app.engine_manager.fetch_rx = Some(github::fetch_releases_async());
            }
            if app.engine_manager.fetching {
                row.colored_label(rgba(pal::TEXT_DIM), "Fetching…");
            }
        });
        ui.spacing(10.0);

        if let Some(err) = app.engine_manager.fetch_error.as_ref() {
            ui.colored_label(rgba(pal::ERROR), &format!("Fetch error: {err}"));
            return;
        }

        if app.engine_manager.releases.is_empty() && !app.engine_manager.fetching {
            ui.colored_label(rgba(pal::TEXT_MUTED), "No releases found.");
            return;
        }

        let installed: std::collections::HashSet<String> =
            app.config.engines.iter().map(|e| e.version.clone()).collect();

        let releases = app.engine_manager.releases.clone();
        let mut to_download: Option<github::GithubRelease> = None;
        let mut to_uninstall: Option<String> = None;
        for release in releases.iter() {
            let already = installed.contains(&release.tag_name);
            release_card(ui, release, already, &mut to_download, &mut to_uninstall);
            ui.spacing(8.0);
        }

        if let Some(release) = to_download
            && let Some(asset) = release.editor_asset()
        {
            let runtime = release.runtime_asset();
            app.engine_manager.download_progress = Some((0, asset.size));
            app.engine_manager.download_rx =
                Some(download::start_download(asset, runtime, &release.tag_name));
        }
        if let Some(version) = to_uninstall {
            uninstall_version(app, &version);
        }

        if let Some((done, total)) = app.engine_manager.download_progress {
            let pct = if total == 0 {
                0.0
            } else {
                (done as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
            };
            ui.spacing(8.0);
            ui.colored_label(
                rgba(pal::TEXT_DIM),
                &format!("Downloading… {:.0}% ({done} / {total} bytes)", pct),
            );
        }
    });
}

fn release_card(
    ui: &mut dyn UiBuilder,
    release: &github::GithubRelease,
    already_installed: bool,
    to_download: &mut Option<github::GithubRelease>,
    to_uninstall: &mut Option<String>,
) {
    let r = ui.panel_rect();
    let card_w = (r[2] - 16.0).max(200.0);
    let card_h = 56.0;
    let alloc = ui.allocate_size([card_w, card_h]);
    let pos = [alloc[0], alloc[1]];

    let accent = if release.prerelease {
        pal::WARNING
    } else if already_installed {
        pal::SUCCESS
    } else {
        pal::PRIMARY_DIM
    };

    ui.paint_rect_filled(pos, [card_w, card_h], rgba(pal::SURFACE3), 6.0);
    ui.paint_rect_stroke(pos, [card_w, card_h], rgba(pal::BORDER), 6.0, 1.0);
    ui.paint_rect_filled([pos[0], pos[1]], [3.0, card_h], rgba(accent), 2.0);
    paint_diamond_filled(ui, [pos[0] + 26.0, pos[1] + card_h * 0.5], 6.0, accent);

    // Version + line 2 (badges or info).
    ui.paint_text_styled(
        [pos[0] + 46.0, pos[1] + 10.0],
        &release.tag_name,
        13.0,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    let bx = pos[0] + 46.0;
    let by = pos[1] + 32.0;
    let mut tag_x = bx;
    if release.prerelease {
        ui.region_at([tag_x, by, 60.0, 18.0], &mut |ui| {
            badge(ui, "pre", tint(pal::WARNING, 0.18), pal::WARNING)
        });
        tag_x += 50.0;
    }
    if already_installed {
        ui.region_at([tag_x, by, 80.0, 18.0], &mut |ui| {
            badge(ui, "installed", tint(pal::SUCCESS, 0.18), pal::SUCCESS)
        });
    } else if let Some(asset) = release.editor_asset() {
        ui.paint_text_styled(
            [tag_x, by + 2.0],
            &format!("{} MB", asset.size / 1_000_000),
            11.0,
            rgba(pal::TEXT_MUTED),
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
    } else {
        ui.paint_text_styled(
            [tag_x, by + 2.0],
            "no editor asset",
            11.0,
            rgba(pal::TEXT_MUTED),
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
    }

    // Right: Uninstall (if installed) or Download (if not, when an asset exists).
    let btn_w = 110.0;
    let btn_h = 28.0;
    let btn_y = pos[1] + (card_h - btn_h) * 0.5;
    let btn_x = pos[0] + card_w - 12.0 - btn_w;
    if already_installed {
        let salt = format!("em-uninstall-{}", release.tag_name);
        let int = ui.interact_rect(&salt, [btn_x, btn_y, btn_w, btn_h]);
        let fill = if int.hovered { tint(pal::ERROR, 0.18) } else { pal::SURFACE };
        ui.paint_rect_filled([btn_x, btn_y], [btn_w, btn_h], rgba(fill), 5.0);
        ui.paint_rect_stroke(
            [btn_x, btn_y],
            [btn_w, btn_h],
            rgba(tint(pal::ERROR, 0.55)),
            5.0,
            1.0,
        );
        let text_color = if int.hovered { pal::ERROR } else { pal::TEXT_DIM };
        let s = ui.measure_text("Uninstall", 12.0, FontFamilyHint::Proportional);
        ui.paint_text_styled(
            [btn_x + (btn_w - s[0]) * 0.5, btn_y + (btn_h - s[1]) * 0.5],
            "Uninstall",
            12.0,
            rgba(text_color),
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        if int.clicked {
            *to_uninstall = Some(release.tag_name.clone());
        }
    } else if release.editor_asset().is_some() {
        let salt = format!("em-dl-{}", release.tag_name);
        let int = ui.interact_rect(&salt, [btn_x, btn_y, btn_w, btn_h]);
        let fill = if int.hovered { pal::SURFACE_ACTIVE } else { pal::SURFACE };
        ui.paint_rect_filled([btn_x, btn_y], [btn_w, btn_h], rgba(fill), 5.0);
        ui.paint_rect_stroke(
            [btn_x, btn_y],
            [btn_w, btn_h],
            rgba(tint(pal::PRIMARY, 0.4)),
            5.0,
            1.0,
        );
        let s = ui.measure_text("Download", 12.0, FontFamilyHint::Proportional);
        ui.paint_text_styled(
            [btn_x + (btn_w - s[0]) * 0.5, btn_y + (btn_h - s[1]) * 0.5],
            "Download",
            12.0,
            rgba(pal::TEXT),
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        if int.clicked {
            *to_download = Some(release.clone());
        }
    }
}

fn show_installed_card(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    card_frame(ui, "Installed engines", pal::ACCENT_CYAN, &mut |ui| {
        if app.config.engines.is_empty() {
            ui.colored_label(rgba(pal::TEXT_MUTED), "No engines installed yet — download one above.");
            return;
        }
        let engines = app.config.engines.clone();
        let mut to_remove: Option<usize> = None;
        for (i, engine) in engines.iter().enumerate() {
            installed_card(ui, &engine.version, &engine.editor_binary, i, &mut to_remove);
            ui.spacing(8.0);
        }
        if let Some(idx) = to_remove
            && idx < app.config.engines.len()
        {
            let engine = app.config.engines[idx].clone();
            uninstall_version(app, &engine.version);
        }
    });
}

fn installed_card(
    ui: &mut dyn UiBuilder,
    version: &str,
    binary_path: &str,
    idx: usize,
    to_remove: &mut Option<usize>,
) {
    let r = ui.panel_rect();
    let card_w = (r[2] - 16.0).max(200.0);
    let card_h = 56.0;
    let alloc = ui.allocate_size([card_w, card_h]);
    let pos = [alloc[0], alloc[1]];
    let accent = pal::SUCCESS;

    ui.paint_rect_filled(pos, [card_w, card_h], rgba(pal::SURFACE3), 6.0);
    ui.paint_rect_stroke(pos, [card_w, card_h], rgba(pal::BORDER), 6.0, 1.0);
    ui.paint_rect_filled([pos[0], pos[1]], [3.0, card_h], rgba(accent), 2.0);
    paint_diamond_filled(ui, [pos[0] + 26.0, pos[1] + card_h * 0.5], 6.0, accent);

    ui.paint_text_styled(
        [pos[0] + 46.0, pos[1] + 10.0],
        version,
        13.0,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.paint_text_styled(
        [pos[0] + 46.0, pos[1] + 32.0],
        binary_path,
        11.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );

    let btn_w = 110.0;
    let btn_h = 28.0;
    let btn_y = pos[1] + (card_h - btn_h) * 0.5;
    let btn_x = pos[0] + card_w - 12.0 - btn_w;
    let salt = format!("em-rm-{}", idx);
    let int = ui.interact_rect(&salt, [btn_x, btn_y, btn_w, btn_h]);
    let fill = if int.hovered { tint(pal::ERROR, 0.18) } else { pal::SURFACE };
    ui.paint_rect_filled([btn_x, btn_y], [btn_w, btn_h], rgba(fill), 5.0);
    ui.paint_rect_stroke(
        [btn_x, btn_y],
        [btn_w, btn_h],
        rgba(tint(pal::ERROR, 0.55)),
        5.0,
        1.0,
    );
    let text_color = if int.hovered { pal::ERROR } else { pal::TEXT_DIM };
    let s = ui.measure_text("Uninstall", 12.0, FontFamilyHint::Proportional);
    ui.paint_text_styled(
        [btn_x + (btn_w - s[0]) * 0.5, btn_y + (btn_h - s[1]) * 0.5],
        "Uninstall",
        12.0,
        rgba(text_color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    if int.clicked {
        *to_remove = Some(idx);
    }
}

/// Removes the engine entry from config AND wipes the install
/// directory on disk. Surfaces the outcome through a banner.
fn uninstall_version(app: &mut HubApp, version: &str) {
    // Remove from config.
    if let Some(idx) = app.config.engines.iter().position(|e| e.version == version) {
        app.config.engines.remove(idx);
        let _ = app.config.save();
    }
    // Wipe the disk install.
    match download::uninstall_engine(version) {
        Ok(()) => {
            app.banner = Some(crate::Banner::info(format!("Uninstalled engine {version}")));
        }
        Err(e) => {
            app.banner = Some(crate::Banner::error(format!(
                "Failed to remove engine {version} on disk: {e}"
            )));
        }
    }
}

/// Auto-sized card frame with a coloured accent stripe in the title
/// row. The accent communicates the section's status (success /
/// primary / warning / cyan).
fn card_frame(
    ui: &mut dyn UiBuilder,
    title: &str,
    accent: khora_sdk::tool_ui::LinearRgba,
    body: &mut dyn FnMut(&mut dyn UiBuilder),
) {
    ui.frame_box(
        Margin::same(14.0),
        Some(pal::SURFACE2),
        Stroke::new(pal::BORDER, 1.0),
        CornerRadius::same(6.0),
        &mut |ui| {
            let pos = ui.cursor_pos();
            ui.paint_rect_filled([pos[0], pos[1] + 2.0], [3.0, 18.0], rgba(accent), 2.0);
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

fn kv_row(ui: &mut dyn UiBuilder, key: &str, value: &str, family: FontFamilyHint) {
    ui.horizontal(&mut |row| {
        let r = row.allocate_size([60.0, 18.0]);
        row.paint_text_styled(
            [r[0], r[1]],
            key,
            11.0,
            rgba(pal::TEXT_MUTED),
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
        row.colored_label(rgba(pal::TEXT_DIM), value);
        let _ = (value, family);
    });
}

fn title_label(ui: &mut dyn UiBuilder, text: &str, size: f32) {
    let r = ui.allocate_size([400.0, size + 6.0]);
    ui.paint_text_styled(
        [r[0], r[1]],
        text,
        size,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    let _ = primary_button; // suppress unused warning until we add primary actions
}
