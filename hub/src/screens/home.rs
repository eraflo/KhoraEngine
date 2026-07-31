// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Home — the project grid and the sidebar.
//!
//! Cards carry their identity with the brand diamond and a clean border. The
//! old left accent-stripe is gone: a coloured bar down the side of a card is
//! decoration pretending to be information, and with a grid of them it reads
//! as noise.

use crate::HubApp;
use crate::Screen;
use crate::config::RecentProject;
use crate::ui::widgets::format_ts;
use khora_sdk::tool_ui::{FontFamilyHint, Icon, Interaction, UiBuilder, UiTheme};
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{
    self, Button, ButtonKind, Tone, button, chip, diamond, empty_state, icon_button, nav_item,
    paint::{display, fill_stroke, mono, text, vertical_gradient},
    search_field,
};

/// Sidebar width — wide enough for the nav labels, narrow enough that the
/// grid still gets three columns on a 1100px window.
const SIDEBAR_W: f32 = 228.0;

const CARD_MIN_W: f32 = 252.0;
const CARD_H: f32 = 104.0;
const GAP: f32 = 12.0;

enum ProjectAction {
    Open(RecentProject),
    AskRemove(usize),
    AddNativeCode(usize),
}

/// Renders the home screen.
pub fn show_home(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    ui.left_inset_panel("hp_sidebar", SIDEBAR_W, &mut |ui| show_sidebar(app, ui));
    ui.central_inset(&mut |ui| show_main(app, ui));

    if let Some(idx) = app.home.remove_confirm {
        show_remove_confirm_modal(app, ui, idx);
    }
}

// ── Sidebar ─────────────────────────────────────────────

fn show_sidebar(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let t = khora_dark();
    let r = ui.panel_rect();

    vertical_gradient(ui, r, t.surface, t.background, 8);
    ui.paint_line(
        [widgets::right(r), r[1]],
        [widgets::right(r), widgets::bottom(r)],
        t.border,
        1.0,
    );

    let pad = 16.0;
    let x = r[0] + pad;
    let w = r[2] - pad * 2.0;
    let mut y = r[1] + pad;

    eyebrow(ui, &t, [x, y], "Projects");
    y += 20.0;

    if button(
        ui,
        &t,
        [x, y, w, 32.0],
        "hp-new",
        Button::new("New project", ButtonKind::Primary).icon(Icon::Plus),
    )
    .clicked
    {
        app.screen = Screen::NewProject;
    }
    y += 32.0 + 8.0;

    if button(
        ui,
        &t,
        [x, y, w, 30.0],
        "hp-open-folder",
        Button::new("Open folder…", ButtonKind::Ghost).icon(Icon::FolderOpen),
    )
    .clicked
        && let Some(path) = rfd::FileDialog::new().pick_folder()
    {
        app.open_existing_project(path);
    }
    y += 30.0 + 24.0;

    eyebrow(ui, &t, [x, y], "View");
    y += 20.0;

    let nav = [
        ("hp-nav-projects", Icon::Layers, "Recent", Screen::Home),
        (
            "hp-nav-engines",
            Icon::Cpu,
            "Engines",
            Screen::EngineManager,
        ),
        (
            "hp-nav-settings",
            Icon::Settings,
            "Settings",
            Screen::Settings,
        ),
    ];
    for (salt, glyph, label, screen) in nav {
        let active = app.screen == screen;
        if nav_item(ui, &t, [x, y, w, 32.0], salt, glyph, label, active).clicked {
            if screen == Screen::Settings {
                app.settings.local_repo_draft =
                    app.config.local_engine_repo.clone().unwrap_or_default();
            }
            app.screen = screen;
        }
        y += 32.0 + 2.0;
    }

    // ── Bottom: where GitHub stands ──
    let gh_h = 30.0;
    let gh_y = widgets::bottom(r) - pad - gh_h;
    let (label, tone) = match &app.settings.auth {
        crate::AuthState::Connected { login, .. } => (format!("@{login}"), Tone::Success),
        crate::AuthState::Connecting { .. } => ("connecting…".to_owned(), Tone::Warning),
        crate::AuthState::Disconnected => ("not connected".to_owned(), Tone::Neutral),
    };
    chip(ui, &t, [x, gh_y + 6.0, w, 18.0], &label, tone, true);
}

fn eyebrow(ui: &mut dyn UiBuilder, t: &UiTheme, pos: [f32; 2], label: &str) {
    mono(
        ui,
        pos,
        &label.to_uppercase(),
        t.font_size_caption - 1.0,
        t.text_disabled,
    );
}

// ── Main ────────────────────────────────────────────────

fn show_main(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let t = khora_dark();
    let r = ui.panel_rect();
    let pad = 24.0;

    // Header: the one display-serif moment on the screen.
    let head_y = r[1] + 20.0;
    display(ui, [r[0] + pad, head_y], "Recent projects", 19.0, t.text);

    let sw = 220.0;
    if search_field(
        ui,
        &t,
        [widgets::right(r) - pad - sw, head_y - 4.0, sw, 30.0],
        "hp-filter",
        &app.home.filter,
        "Filter projects…",
    )
    .clicked
    {
        // The frame is painted here; the real text edit lives behind it.
    }

    let body_y = head_y + 34.0;
    let body = [
        r[0] + pad,
        body_y,
        r[2] - pad * 2.0,
        widgets::bottom(r) - body_y - pad,
    ];

    if app.config.recent_projects.is_empty() {
        let w = 340.0f32.min(body[2]);
        let h = 170.0;
        let e = [body[0] + (body[2] - w) * 0.5, body[1] + 24.0, w, h];
        empty_state(
            ui,
            &t,
            e,
            Icon::Layers,
            "No projects yet",
            "Create your first Khora project, or open an existing folder.",
        );
        if button(
            ui,
            &t,
            [
                e[0] + (w - 150.0) * 0.5,
                widgets::bottom(e) + 14.0,
                150.0,
                32.0,
            ],
            "hp-empty-new",
            Button::new("New project", ButtonKind::Primary).icon(Icon::Plus),
        )
        .clicked
        {
            app.screen = Screen::NewProject;
        }
        return;
    }

    // Filter, keeping the original index so actions still address the right
    // project after the list is narrowed.
    let needle = app.home.filter.to_ascii_lowercase();
    let projects: Vec<(usize, RecentProject)> = app
        .config
        .recent_projects
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, p)| {
            needle.is_empty()
                || p.name.to_ascii_lowercase().contains(&needle)
                || p.path.to_ascii_lowercase().contains(&needle)
        })
        .collect();

    if projects.is_empty() {
        text(
            ui,
            [body[0], body[1] + 8.0],
            "No projects match your filter.",
            t.font_size_body,
            t.text_muted,
        );
        return;
    }

    // Responsive grid: as many columns as fit at the minimum card width.
    let cols = (((body[2] + GAP) / (CARD_MIN_W + GAP)).floor() as usize).max(1);
    let card_w = (body[2] - GAP * (cols - 1) as f32) / cols as f32;

    let mut action: Option<ProjectAction> = None;
    for (i, (src_idx, proj)) in projects.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let rect = [
            body[0] + col as f32 * (card_w + GAP),
            body[1] + row as f32 * (CARD_H + GAP),
            card_w,
            CARD_H,
        ];
        if widgets::bottom(rect) > widgets::bottom(body) {
            break; // Off-screen; the panel doesn't scroll yet.
        }
        project_card(ui, &t, rect, proj, *src_idx, &mut action);
    }

    apply_action(app, action);
}

fn project_card(
    ui: &mut dyn UiBuilder,
    t: &UiTheme,
    rect: [f32; 4],
    proj: &RecentProject,
    idx: usize,
    action: &mut Option<ProjectAction>,
) -> Interaction {
    // The whole card is the Open affordance. Allocate it first so the icon
    // buttons painted later win the click.
    let card = ui.interact_rect(&format!("hp-card-{idx}"), rect);

    let (bg, border) = if card.hovered {
        (t.surface_elevated, t.border_strong)
    } else {
        (t.surface, t.border)
    };
    fill_stroke(ui, rect, bg, border, t.radius_md);

    let pad = 16.0;
    let x = rect[0] + pad;

    // Header: diamond, name, version.
    let cy = rect[1] + pad + 7.0;
    diamond(
        ui,
        [x + 7.0, cy],
        14.0,
        if card.hovered {
            t.primary
        } else {
            t.primary_dim
        },
    );

    let name_x = x + 24.0;
    let ver = proj.engine_version.as_str();
    let ver_w = ui.measure_text(ver, t.font_size_caption, FontFamilyHint::Monospace)[0] + 18.0;
    chip(
        ui,
        t,
        [widgets::right(rect) - pad - ver_w, cy - 9.0, ver_w, 18.0],
        ver,
        Tone::Neutral,
        false,
    );

    let name_w = (widgets::right(rect) - pad - ver_w - 8.0 - name_x).max(0.0);
    ui.region_at("project-card-name", [name_x, cy - 8.0, name_w, 16.0], &mut |ui| {
        text(
            ui,
            [name_x, cy - 7.0],
            &proj.name,
            t.font_size_title,
            t.text,
        );
    });

    // Path — data, so monospace.
    mono(
        ui,
        [x, rect[1] + 44.0],
        &proj.path,
        t.font_size_caption,
        t.text_disabled,
    );

    // ── Footer ──
    let fy = widgets::bottom(rect) - pad - 9.0;

    text(
        ui,
        [x, fy - 7.0],
        "Open",
        t.font_size_caption + 1.0,
        t.primary,
    );

    // Row actions replace the timestamp on hover — they are rare, and a card
    // grid should read as content, not as a wall of buttons.
    if card.hovered {
        let bw = 26.0;
        let mut bx = widgets::right(rect) - pad - bw;

        if icon_button(
            ui,
            t,
            [bx, fy - 13.0, bw, 26.0],
            &format!("hp-rm-{idx}"),
            Icon::Trash,
            true,
        )
        .clicked
        {
            *action = Some(ProjectAction::AskRemove(idx));
        }
        bx -= bw + 2.0;

        if !crate::project::has_native_code(std::path::Path::new(&proj.path))
            && icon_button(
                ui,
                t,
                [bx, fy - 13.0, bw, 26.0],
                &format!("hp-native-{idx}"),
                Icon::Code,
                true,
            )
            .clicked
        {
            *action = Some(ProjectAction::AddNativeCode(idx));
        }
    } else {
        let ts = format_ts(proj.last_opened);
        let w = ui.measure_text(&ts, t.font_size_caption, FontFamilyHint::Monospace)[0];
        mono(
            ui,
            [widgets::right(rect) - pad - w, fy - 7.0],
            &ts,
            t.font_size_caption,
            t.text_muted,
        );
    }

    if card.clicked {
        *action = Some(ProjectAction::Open(proj.clone()));
    }
    card
}

fn apply_action(app: &mut HubApp, action: Option<ProjectAction>) {
    match action {
        Some(ProjectAction::Open(proj)) => app.launch_project(&proj),
        Some(ProjectAction::AskRemove(idx)) => app.home.remove_confirm = Some(idx),
        Some(ProjectAction::AddNativeCode(idx)) => {
            let Some(proj) = app.config.recent_projects.get(idx).cloned() else {
                return;
            };
            let root = std::path::PathBuf::from(&proj.path);
            match crate::project::add_native_code(&root, &proj.name, &proj.engine_version) {
                Ok(()) => {
                    app.banner = Some(crate::Banner::info(format!(
                        "Added a native Rust scaffold to '{}'.",
                        proj.name
                    )));
                }
                Err(e) => {
                    app.banner = Some(crate::Banner::error(format!(
                        "Couldn't add native code: {e:#}"
                    )));
                }
            }
        }
        None => {}
    }
}

// ── Destructive confirm ─────────────────────────────────

fn show_remove_confirm_modal(app: &mut HubApp, ui: &mut dyn UiBuilder, idx: usize) {
    let Some(proj) = app.config.recent_projects.get(idx).cloned() else {
        app.home.remove_confirm = None;
        return;
    };
    let t = khora_dark();

    let mut delete = false;
    let mut close = false;

    ui.modal("hp-remove-confirm", [420.0, 190.0], &mut |ui| {
        let r = ui.panel_rect();
        let pad = 20.0;
        let x = r[0] + pad;

        text(
            ui,
            [x, r[1] + pad],
            &format!("Delete “{}” from disk?", proj.name),
            t.font_size_title,
            t.text,
        );
        mono(
            ui,
            [x, r[1] + pad + 26.0],
            &proj.path,
            t.font_size_caption,
            t.text_muted,
        );
        text(
            ui,
            [x, r[1] + pad + 48.0],
            "This removes the folder and everything in it. It cannot be undone.",
            t.font_size_caption + 1.0,
            t.warning,
        );

        let by = widgets::bottom(r) - pad - 32.0;
        let bw = 96.0;
        if button(
            ui,
            &t,
            [widgets::right(r) - pad - bw, by, bw, 32.0],
            "hp-modal-delete",
            Button::new("Delete", ButtonKind::Danger).icon(Icon::Trash),
        )
        .clicked
        {
            delete = true;
        }
        if button(
            ui,
            &t,
            [widgets::right(r) - pad - bw * 2.0 - 8.0, by, bw, 32.0],
            "hp-modal-cancel",
            Button::new("Cancel", ButtonKind::Ghost),
        )
        .clicked
        {
            close = true;
        }
    });

    if delete {
        let path = std::path::PathBuf::from(&proj.path);
        if path.exists() {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    app.banner = Some(crate::Banner::info(format!("Deleted '{}'", proj.name)))
                }
                Err(e) => {
                    app.banner = Some(crate::Banner::error(format!(
                        "Couldn't delete '{}': {e}",
                        proj.path
                    )))
                }
            }
        }
        app.config.recent_projects.remove(idx);
        let _ = app.config.save();
        app.home.remove_confirm = None;
    } else if close {
        app.home.remove_confirm = None;
    }
}
