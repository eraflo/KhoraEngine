// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Inspector asset-metadata pane.
//!
//! When the user selects an asset in the browser, the Properties panel
//! switches from per-entity component cards to this pane: file
//! metadata, source path, and a couple of OS-level actions
//! (Reveal in Explorer, Open Externally). Phase 5 entry — per-type
//! preview cards (texture image, mesh stats, audio duration) come in
//! a follow-up.

use khora_sdk::editor_ui::{UiTheme, FontFamilyHint, Icon, TextAlign, UiBuilder};

use super::header::paint_inspector_header;
use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

/// One row of "key — value" inside the metadata card.
fn paint_kv_row(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    key: &str,
    value: &str,
    theme: &UiTheme,
) {
    ui.paint_text_styled(
        [origin[0], origin[1]],
        key,
        11.0,
        theme.text_muted,
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    ui.paint_text_styled(
        [origin[0] + width - 6.0, origin[1]],
        value,
        11.0,
        theme.text,
        FontFamilyHint::Monospace,
        TextAlign::Right,
    );
}

/// Render the asset metadata pane. Returns the y coordinate after the
/// last painted block.
pub fn render_asset_pane(
    ui: &mut dyn UiBuilder,
    body_rect: [f32; 4],
    rel_path: &str,
    project_folder: &str,
    theme: &UiTheme,
) {
    let [bx, by, bw, _] = body_rect;
    let mut y = by;

    // Resolve the absolute path + on-disk metadata.
    let abs = std::path::Path::new(project_folder)
        .join("assets")
        .join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let meta = std::fs::metadata(&abs).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        });
    let extension = std::path::Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // ── Card: file info ────────────────────────────────
    let card_x = bx;
    let card_w = bw;
    let card_h = 130.0;
    ui.paint_rect_filled(
        [card_x, y],
        [card_w, card_h],
        theme.surface_elevated,
        theme.radius_md,
    );
    ui.paint_rect_stroke(
        [card_x, y],
        [card_w, card_h],
        with_alpha(theme.separator, 0.55),
        theme.radius_md,
        1.0,
    );
    paint_text_size(ui, [card_x + 12.0, y + 10.0], "File", 12.0, theme.text);

    let row_x = card_x + 12.0;
    let row_w = card_w - 24.0;
    let mut row_y = y + 32.0;
    paint_kv_row(ui, [row_x, row_y], row_w, "PATH", rel_path, theme);
    row_y += 18.0;
    paint_kv_row(
        ui,
        [row_x, row_y],
        row_w,
        "TYPE",
        extension.to_uppercase().as_str(),
        theme,
    );
    row_y += 18.0;
    paint_kv_row(
        ui,
        [row_x, row_y],
        row_w,
        "SIZE",
        &format_size(size),
        theme,
    );
    row_y += 18.0;
    paint_kv_row(
        ui,
        [row_x, row_y],
        row_w,
        "MTIME",
        &format_mtime(modified),
        theme,
    );

    y += card_h + 10.0;

    // ── Card: actions ──────────────────────────────────
    let actions_h = 40.0;
    ui.paint_rect_filled(
        [card_x, y],
        [card_w, actions_h],
        theme.surface_elevated,
        theme.radius_md,
    );
    ui.paint_rect_stroke(
        [card_x, y],
        [card_w, actions_h],
        with_alpha(theme.separator, 0.55),
        theme.radius_md,
        1.0,
    );
    let abs_string = abs.to_string_lossy().to_string();
    let buttons = [
        ("Open Externally", Icon::More, "asset-open-ext"),
        ("Reveal in Folder", Icon::Database, "asset-reveal"),
    ];
    let btn_w = (card_w - 24.0 - 8.0) / 2.0;
    for (i, (label, icon, salt)) in buttons.iter().enumerate() {
        let bx = card_x + 12.0 + i as f32 * (btn_w + 8.0);
        let by_btn = y + 8.0;
        let int = ui.interact_rect(salt, [bx, by_btn, btn_w, 24.0]);
        let bg = if int.hovered {
            theme.surface_active
        } else {
            theme.background
        };
        ui.paint_rect_filled([bx, by_btn], [btn_w, 24.0], bg, theme.radius_sm);
        paint_icon(ui, [bx + 8.0, by_btn + 6.0], *icon, 12.0, theme.text_dim);
        paint_text_size(ui, [bx + 28.0, by_btn + 6.0], label, 11.0, theme.text);
        if int.clicked {
            match *salt {
                "asset-open-ext" => match open::that(&abs_string) {
                    Ok(()) => log::info!("asset pane: opened '{}'", rel_path),
                    Err(e) => log::warn!(
                        "asset pane: failed to open '{}' externally: {}",
                        rel_path,
                        e
                    ),
                },
                "asset-reveal" => {
                    let parent = abs.parent().map(|p| p.to_path_buf());
                    if let Some(parent) = parent {
                        let _ = open::that(&parent);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Header line for asset mode — re-uses the entity Inspector header
/// widget so the visual rhythm matches.
pub fn paint_asset_header(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    width: f32,
    rel_path: &str,
    theme: &UiTheme,
) -> f32 {
    let name = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(rel_path)
        .to_string();
    let extension = std::path::Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("file")
        .to_uppercase();
    paint_inspector_header(
        ui,
        origin,
        width,
        Icon::Database,
        &name,
        &extension,
        "Asset",
        theme.primary,
        None,
        theme,
    )
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}

fn format_mtime(secs: Option<u64>) -> String {
    let Some(secs) = secs else {
        return "—".into();
    };
    // Rough compact `YYYY-MM-DD` from Unix seconds — avoids pulling in
    // chrono just for an Inspector display.
    let days_since_epoch = (secs / 86_400) as i64;
    let (y, m, d) = days_to_ymd(days_since_epoch);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Convert days since the Unix epoch to (year, month, day). Used only
/// for the Inspector mtime label.
fn days_to_ymd(mut days: i64) -> (i32, u32, u32) {
    days += 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = (doe - (365 * yoe + yoe / 4 - yoe / 100)) as u32;
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
